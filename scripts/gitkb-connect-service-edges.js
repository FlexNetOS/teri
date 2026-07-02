#!/usr/bin/env bun

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";

const root = process.cwd();
const dbPath = path.join(root, ".kb", ".cache", "gitkb.db");

if (!existsSync(dbPath)) {
  throw new Error("GitKB code database not found at .kb/.cache/gitkb.db; run git-kb code index first");
}

function sqliteJson(sql) {
  const result = spawnSync("sqlite3", ["-json", dbPath, sql], { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "sqlite3 query failed");
  }
  return result.stdout.trim() ? JSON.parse(result.stdout) : [];
}

function sqliteExec(sql) {
  const result = spawnSync("sqlite3", [dbPath], { input: sql, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || result.stdout.trim() || "sqlite3 exec failed");
  }
}

function sqlString(value) {
  if (value === null || value === undefined) return "NULL";
  return "'" + String(value).replaceAll("'", "''") + "'";
}

function lineOf(source, index) {
  let line = 1;
  for (let i = 0; i < index; i += 1) {
    if (source.charCodeAt(i) === 10) line += 1;
  }
  return line;
}

function lineBounds(source, line) {
  let start = 0;
  let current = 1;
  while (current < line && start < source.length) {
    const next = source.indexOf("\n", start);
    if (next === -1) return [source.length, source.length];
    start = next + 1;
    current += 1;
  }
  const end = source.indexOf("\n", start);
  return [start, end === -1 ? source.length : end];
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function normalizeHttpPath(raw) {
  let value = raw
    .replace(/\$\{[^}]+\}/g, ":param")
    .replace(/^https?:\/\/[^/]+/i, "")
    .replace(/[?#].*$/, "")
    .trim();
  if (!value.startsWith("/")) value = "/" + value;
  value = value.replace(/\/+/g, "/");
  value = value.replace(/\/:(?:[A-Za-z_][A-Za-z0-9_]*|param)(?=\/|$)/g, "/:param");
  return value.length > 1 && value.endsWith("/") ? value.slice(0, -1) : value;
}

function joinRoute(prefix, routePath) {
  const left = normalizeHttpPath(prefix);
  const right = normalizeHttpPath(routePath);
  if (right === "/") return left;
  if (left === "/") return right;
  return normalizeHttpPath(left + "/" + right.slice(1));
}

function stableId(prefix, parts) {
  return prefix + "-" + createHash("sha256").update(parts.join("\0")).digest("hex").slice(0, 24);
}

function stableUuid(parts) {
  const hex = createHash("sha256").update(parts.join("\0")).digest("hex");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20, 32),
  ].join("-");
}

function enclosingSymbol(symbols, filePath, line) {
  return symbols
    .filter((symbol) => symbol.file_path === filePath)
    .filter((symbol) => line >= symbol.line_range_start && line <= symbol.line_range_end)
    .sort((a, b) => {
      const spanA = a.line_range_end - a.line_range_start;
      const spanB = b.line_range_end - b.line_range_start;
      return spanA - spanB;
    })[0]?.symbol_id ?? null;
}

function syntheticClientSymbol(filePath, source, line, sourceKind) {
  const [start, end] = lineBounds(source, line);
  const snippet = source.slice(start, end).trim();
  const symbolId = [
    filePath,
    "connector_client",
    String(line),
    sha256(snippet).slice(0, 12),
  ].join("::");
  return {
    symbol_id: symbolId,
    name: "connector_client:" + path.basename(filePath) + ":" + line,
    kind: "connector_client",
    file_path: filePath,
    byte_range_start: Buffer.byteLength(source.slice(0, start)),
    byte_range_end: Buffer.byteLength(source.slice(0, end)),
    line_range_start: line,
    line_range_end: line,
    signature: sourceKind + " " + snippet,
    content_hash: sha256(snippet),
    file_content_hash: sha256(source),
    language: path.extname(filePath).slice(1) || "unknown",
  };
}

function extractClientCalls(filePath, source, symbols) {
  const calls = [];
  const seen = new Set();

  function push(method, rawPath, index, sourceKind) {
    const normalizedPath = normalizeHttpPath(rawPath);
    const line = lineOf(source, index);
    const callerSymbol = enclosingSymbol(symbols, filePath, line);
    const syntheticSymbol = callerSymbol ? null : syntheticClientSymbol(filePath, source, line, sourceKind);
    const key = method + "|" + normalizedPath + "|" + line;
    if (seen.has(key)) return;
    seen.add(key);
    calls.push({
      client_call_id: stableId("teri-js-client", [filePath, String(line), method, normalizedPath]),
      caller_symbol_id: callerSymbol ?? syntheticSymbol.symbol_id,
      synthetic_symbol: syntheticSymbol,
      method,
      url_or_path: rawPath,
      normalized_path: normalizedPath,
      target_service: "teri-http",
      file_path: filePath,
      line,
      source: sourceKind,
      confidence: 0.86,
      reason: "teri_frontend_api_connector",
    });
  }

  for (const match of source.matchAll(/service\.(get|post|put|delete|patch)\s*\(\s*([\x60'"])([^\x60'"]+)\2/g)) {
    push(match[1].toUpperCase(), match[3], match.index ?? 0, "axios-instance-method");
  }

  const namedMethod = {
    apiGet: "GET",
    apiPost: "POST",
    apiDelete: "DELETE",
  };
  for (const match of source.matchAll(/\b(apiGet|apiPost|apiDelete)\s*\(\s*([\x60'"])([^\x60'"]+)\2/g)) {
    push(namedMethod[match[1]], match[3], match.index ?? 0, "named-api-connector");
  }

  for (const match of source.matchAll(/openSse\s*\(\s*([\x60'"])([^\x60'"]+)\1/g)) {
    push("GET", match[2], match.index ?? 0, "eventsource-helper");
  }

  for (const match of source.matchAll(/\b(?:service|apiRequest)\s*\(\s*\{/g)) {
    const index = match.index ?? 0;
    const window = source.slice(index, index + 900);
    const url = window.match(/url\s*:\s*([\x60'"])([^\x60'"]+)\1/);
    if (!url) continue;
    const method = window.match(/method\s*:\s*([\x60'"])([^\x60'"]+)\1/);
    push((method?.[2] ?? "GET").toUpperCase(), url[2], index, "axios-config-object");
  }

  return calls;
}

function collectFrontendFiles(dir) {
  if (!existsSync(path.join(root, dir))) return [];
  return readdirSync(path.join(root, dir), { withFileTypes: true }).flatMap((entry) => {
    const relativePath = path.join(dir, entry.name);
    if (entry.isDirectory()) return collectFrontendFiles(relativePath);
    if (!/\.(js|vue)$/.test(entry.name)) return [];
    return [relativePath];
  });
}

const routes = sqliteJson(
  "SELECT route_id, method, path, normalized_path, handler_name, handler_symbol_id, framework, file_path, line FROM code_route WHERE branch = 'main'"
);
const mounts = sqliteJson(
  "SELECT prefix, normalized_prefix, target_hint, file_path, line FROM code_service_mount WHERE branch = 'main'"
);
const symbols = sqliteJson(
  "SELECT symbol_id, name, kind, file_path, line_range_start, line_range_end FROM code_symbol WHERE branch = 'main'"
);

const symbolIds = new Set(symbols.map((symbol) => symbol.symbol_id));

const apiMount = mounts.find((mount) => mount.target_hint === "api_router")?.normalized_prefix ?? "";
const moduleMounts = new Map();
for (const mount of mounts) {
  const module = mount.target_hint.match(/crate::api::([A-Za-z0-9_]+)::/);
  if (module) moduleMounts.set(module[1], mount.normalized_prefix);
}

function routeFullPath(route) {
  const apiModule = route.file_path.match(/^src\/api\/([A-Za-z0-9_]+)\.rs$/)?.[1];
  if (apiModule && moduleMounts.has(apiModule)) {
    return joinRoute(joinRoute(apiMount || "/", moduleMounts.get(apiModule)), route.normalized_path);
  }
  return normalizeHttpPath(route.normalized_path);
}

const routeByMethodPath = new Map();
for (const route of routes) {
  if (!route.method) continue;
  const fullPath = routeFullPath(route);
  const key = route.method.toUpperCase() + " " + fullPath;
  if (!routeByMethodPath.has(key)) routeByMethodPath.set(key, []);
  routeByMethodPath.get(key).push({ ...route, fullPath });
}

const routerByFile = new Map([
  ["pebesen/crates/intelligence/src/http.rs", "pebesen/crates/intelligence/src/http.rs::function::router"],
  ["src/server.rs", "src/server.rs::function::create_app"],
  ["src/api/graph.rs", "src/api/graph.rs::function::graph_router"],
  ["src/api/prompt_templates.rs", "src/api/prompt_templates.rs::function::prompt_templates_router"],
  ["src/api/report.rs", "src/api/report.rs::function::report_router"],
  ["src/api/simulation.rs", "src/api/simulation.rs::function::simulation_router"],
  ["src/api/templates.rs", "src/api/templates.rs::function::templates_router"],
]);

function routeExposureSource(route) {
  const routerSymbol = routerByFile.get(route.file_path);
  if (routerSymbol && symbolIds.has(routerSymbol)) return routerSymbol;
  return null;
}

const frontendFiles = collectFrontendFiles("frontend/src");

const clients = frontendFiles
  .filter((filePath) => existsSync(path.join(root, filePath)))
  .flatMap((filePath) => extractClientCalls(filePath, readFileSync(path.join(root, filePath), "utf8"), symbols));

let clientRows = 0;
let edgeRows = 0;
let unmatchedClients = 0;
let routeExposureRows = 0;
let unmatchedRouteExposure = 0;
const statements = ["BEGIN;"];

for (const client of clients) {
  if (client.synthetic_symbol) {
    const symbol = client.synthetic_symbol;
    statements.push("INSERT INTO code_symbol (" +
      "symbol_id, branch, name, kind, file_path, byte_range_start, byte_range_end, " +
      "line_range_start, line_range_end, parent, signature, content_hash, file_content_hash, " +
      "language, visibility, doc_comment, indexed_at, caller_count" +
    ") VALUES (" +
      sqlString(symbol.symbol_id) + ", 'main', " + sqlString(symbol.name) + ", " +
      sqlString(symbol.kind) + ", " + sqlString(symbol.file_path) + ", " +
      symbol.byte_range_start + ", " + symbol.byte_range_end + ", " +
      symbol.line_range_start + ", " + symbol.line_range_end + ", NULL, " +
      sqlString(symbol.signature) + ", " + sqlString(symbol.content_hash) + ", " +
      sqlString(symbol.file_content_hash) + ", " + sqlString(symbol.language) + ", " +
      "'private', 'materialized by scripts/gitkb-connect-service-edges.js for real frontend endpoint call site', " +
      "datetime('now'), 0" +
    ") ON CONFLICT(symbol_id, branch) DO UPDATE SET " +
      "name = excluded.name, " +
      "kind = excluded.kind, " +
      "file_path = excluded.file_path, " +
      "byte_range_start = excluded.byte_range_start, " +
      "byte_range_end = excluded.byte_range_end, " +
      "line_range_start = excluded.line_range_start, " +
      "line_range_end = excluded.line_range_end, " +
      "signature = excluded.signature, " +
      "content_hash = excluded.content_hash, " +
      "file_content_hash = excluded.file_content_hash, " +
      "language = excluded.language, " +
      "visibility = excluded.visibility, " +
      "doc_comment = excluded.doc_comment, " +
      "indexed_at = excluded.indexed_at;");
  }

  statements.push("INSERT INTO code_client_call (" +
    "client_call_id, branch, caller_symbol_id, method, url_or_path, normalized_path, " +
    "target_service, file_path, line, column, byte_start, byte_end, source, confidence, reason" +
  ") VALUES (" +
    sqlString(client.client_call_id) + ", 'main', " + sqlString(client.caller_symbol_id) + ", " +
    sqlString(client.method) + ", " + sqlString(client.url_or_path) + ", " + sqlString(client.normalized_path) + ", " +
    sqlString(client.target_service) + ", " + sqlString(client.file_path) + ", " + client.line + ", " +
    "NULL, NULL, NULL, " + sqlString(client.source) + ", " + client.confidence + ", " + sqlString(client.reason) +
  ") ON CONFLICT(branch, client_call_id) DO UPDATE SET " +
    "caller_symbol_id = excluded.caller_symbol_id, " +
    "method = excluded.method, " +
    "url_or_path = excluded.url_or_path, " +
    "normalized_path = excluded.normalized_path, " +
    "target_service = excluded.target_service, " +
    "file_path = excluded.file_path, " +
    "line = excluded.line, " +
    "source = excluded.source, " +
    "confidence = excluded.confidence, " +
    "reason = excluded.reason, " +
    "updated_at = datetime('now');");
  clientRows += 1;

  const matches = routeByMethodPath.get(client.method + " " + client.normalized_path) ?? [];
  if (matches.length === 0 || !client.caller_symbol_id) {
    unmatchedClients += 1;
    continue;
  }

  for (const route of matches) {
    const edgeId = stableUuid([
      client.client_call_id,
      route.route_id,
      client.caller_symbol_id,
      client.method,
      client.normalized_path,
    ]);
    statements.push("INSERT INTO code_service_edge (" +
      "id, branch, source_symbol_id, target, edge_type, file_path, line, confidence, reason, " +
      "provenance_source, target_route_id, client_call_id, method, normalized_path, match_kind, match_reason" +
    ") VALUES (" +
      sqlString(edgeId) + ", 'main', " + sqlString(client.caller_symbol_id) + ", " +
      sqlString(route.handler_symbol_id ?? route.handler_name ?? route.fullPath) + ", " +
      "'http_client_to_route', " + sqlString(client.file_path) + ", " + client.line + ", 0.91, " +
      "'teri_frontend_matches_axum_route', 'scripts/gitkb-connect-service-edges.js', " +
      sqlString(route.route_id) + ", " + sqlString(client.client_call_id) + ", " +
      sqlString(client.method) + ", " + sqlString(client.normalized_path) + ", 'method_and_normalized_path', " +
      sqlString(client.method + " " + client.normalized_path + " -> " + route.file_path + ":" + route.line) +
    ") ON CONFLICT(id) DO UPDATE SET " +
      "source_symbol_id = excluded.source_symbol_id, " +
      "target = excluded.target, " +
      "edge_type = excluded.edge_type, " +
      "file_path = excluded.file_path, " +
      "line = excluded.line, " +
      "confidence = excluded.confidence, " +
      "reason = excluded.reason, " +
      "provenance_source = excluded.provenance_source, " +
      "target_route_id = excluded.target_route_id, " +
      "client_call_id = excluded.client_call_id, " +
      "method = excluded.method, " +
      "normalized_path = excluded.normalized_path, " +
      "match_kind = excluded.match_kind, " +
      "match_reason = excluded.match_reason;");
    edgeRows += 1;
  }
}

for (const route of routes) {
  const sourceSymbolId = routeExposureSource(route);
  const fullPath = routeFullPath(route);
  if (!sourceSymbolId || !route.handler_symbol_id) {
    unmatchedRouteExposure += 1;
    continue;
  }

  const edgeId = stableUuid([
    "route-exposure",
    sourceSymbolId,
    route.route_id,
    route.handler_symbol_id,
    route.method ?? "",
    fullPath,
  ]);
  statements.push("INSERT INTO code_service_edge (" +
    "id, branch, source_symbol_id, target, edge_type, file_path, line, confidence, reason, " +
    "provenance_source, target_route_id, client_call_id, method, normalized_path, match_kind, match_reason" +
  ") VALUES (" +
    sqlString(edgeId) + ", 'main', " + sqlString(sourceSymbolId) + ", " +
    sqlString(route.handler_symbol_id) + ", " +
    "'router_exposes_route', " + sqlString(route.file_path) + ", " + route.line + ", 0.94, " +
    "'teri_router_exposes_axum_route', 'scripts/gitkb-connect-service-edges.js', " +
    sqlString(route.route_id) + ", NULL, " + sqlString(route.method?.toUpperCase() ?? null) + ", " +
    sqlString(fullPath) + ", 'router_function_to_handler_route', " +
    sqlString((route.method ?? "ANY").toUpperCase() + " " + fullPath + " exposed by " + sourceSymbolId + " -> " + route.handler_symbol_id) +
  ") ON CONFLICT(id) DO UPDATE SET " +
    "source_symbol_id = excluded.source_symbol_id, " +
    "target = excluded.target, " +
    "edge_type = excluded.edge_type, " +
    "file_path = excluded.file_path, " +
    "line = excluded.line, " +
    "confidence = excluded.confidence, " +
    "reason = excluded.reason, " +
    "provenance_source = excluded.provenance_source, " +
    "target_route_id = excluded.target_route_id, " +
    "client_call_id = excluded.client_call_id, " +
    "method = excluded.method, " +
    "normalized_path = excluded.normalized_path, " +
    "match_kind = excluded.match_kind, " +
    "match_reason = excluded.match_reason;");
  routeExposureRows += 1;
}

statements.push("COMMIT;");
sqliteExec(statements.join("\n"));

const health = sqliteJson("SELECT " +
  "(SELECT COUNT(*) FROM code_route WHERE branch = 'main') AS route_count, " +
  "(SELECT COUNT(*) FROM code_client_call WHERE branch = 'main') AS client_call_count, " +
  "(SELECT COUNT(*) FROM code_service_edge WHERE branch = 'main') AS matched_service_edge_count, " +
  "(SELECT COUNT(*) FROM code_route r WHERE r.branch = 'main' AND NOT EXISTS (" +
    "SELECT 1 FROM code_service_edge e WHERE e.branch = r.branch AND e.target_route_id = r.route_id" +
  ")) AS unmatched_route_count, " +
  "(SELECT COUNT(*) FROM code_client_call c WHERE c.branch = 'main' AND NOT EXISTS (" +
    "SELECT 1 FROM code_service_edge e WHERE e.branch = c.branch AND e.client_call_id = c.client_call_id" +
  ")) AS unmatched_client_call_count");

console.log(JSON.stringify({
  scanned_files: frontendFiles.length,
  extracted_clients: clients.length,
  materialized_synthetic_symbols: clients.filter((client) => client.synthetic_symbol).length,
  materialized_client_rows: clientRows,
  materialized_client_service_edges: edgeRows,
  materialized_route_exposure_edges: routeExposureRows,
  unmatched_extracted_clients: unmatchedClients,
  unmatched_route_exposures: unmatchedRouteExposure,
  service_edge_health: health[0] ?? null,
}, null, 2));
