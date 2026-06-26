<div align="center">

# Teri

简洁通用的群体智能引擎，预测万物
</br>
<em>A Simple and Universal Swarm Intelligence Engine, Predicting Anything</em>

[English](./README.md) | [中文文档](./README-ZH.md)

</div>

> **Teri** 是 [MiroFish](https://github.com/666ghj/MiroFish)（上游 AGPL-3.0）的 Rust 原生重写版本。
> Teri 是独立的 **MIT** 重新实现——按规格对齐，绝不照抄代码。它是*升级*而非降级：MiroFish 的每一项能力
> 在这里都是硬性要求。"Teri"（印尼语 *ikan teri*，凤尾鱼）是海中最小的鱼之一，却以庞大而紧密协调的鱼群
> 游动，涌现出无单条鱼所能规划的群体行为。这正是本引擎所做的：播下种子、生成蜂群、见证涌现。

## ⚡ 项目概述

**Teri** 是一款基于多智能体技术的新一代 AI 预测引擎。通过提取现实世界的种子信息（如突发新闻、政策草案、
金融信号），自动构建出高保真的平行数字世界。在此空间内，成千上万个具备独立人格、长期记忆与行为逻辑的智能体
进行自由交互与社会演化。你可透过「上帝视角」动态注入变量，精准推演未来走向——**让未来在数字沙盘中预演，
助决策在百战模拟后胜出**。

> 你只需：上传种子材料（数据分析报告或者有趣的小说故事），并用自然语言描述预测需求。</br>
> Teri 将返回：一份详尽的预测报告，以及一个可深度交互的高保真数字世界。

### 我们的愿景

Teri 致力于打造映射现实的群体智能镜像，通过捕捉个体互动引发的群体涌现，突破传统预测的局限：

- **于宏观**：我们是决策者的预演实验室，让政策与公关在零风险中试错。
- **于微观**：我们是个人用户的创意沙盘，无论是推演小说结局还是探索脑洞，皆可有趣、好玩、触手可及。

从严肃预测到趣味仿真，我们让每一个「如果」都能看见结果，让预测万物成为可能。

## 🔄 工作流程

1. **图谱构建**：现实种子提取 & 个体与群体记忆注入 & GraphRAG 构建。
2. **环境搭建**：实体关系抽取 & 人设生成 & 环境配置 Agent 注入仿真参数。
3. **开始模拟**：双平台并行模拟 & 自动解析预测需求 & 动态更新时序记忆。
4. **报告生成**：ReportAgent 拥有丰富的工具集，与模拟后环境进行深度交互。
5. **深度互动**：与模拟世界中的任意一位智能体对话 & 与 ReportAgent 对话。

## ✨ 为什么是 Teri（相对 MiroFish 的升级）

| 关注点 | MiroFish（Python） | Teri（Rust） |
| --- | --- | --- |
| 后端运行时 | Python ≥3.11、uv、Docker + venv | 单一静态二进制（`cargo build --release`） |
| 智能体并发 | 受 GIL 限制的线程 | `tokio` 有界并发 + `rayon` CPU 并行 |
| 时序记忆 / 图谱 | 外部 **Zep Cloud**（`ZEP_API_KEY`） | **原生进程内**时序图谱记忆（petgraph + redb）——无外部服务、无额外密钥 |
| 类型安全 | 运行时错误 | 编译期保证 |
| 密钥 | `.env` 中的明文 API Key | envctl 保险库注入（仅子进程环境）；本地开发用 `.env` |
| 后端诚实性 | — | 预检守卫在任何运行前拒绝桩 / 罐头推理后端 |

## 🚀 快速开始

Teri 有两个界面：**引擎**（Rust——CLI `teri run` + REST/SSE 服务 `teri serve`），以及 **Web UI**
（Vue 3 单页应用——五步预测工作室）。

### 前置要求

| 工具 | 版本要求 | 说明 | 安装检查 |
| --- | --- | --- | --- |
| **Rust** | stable（edition 2024） | 引擎运行环境 | `cargo --version` |
| **Node.js** | 18+ | Web UI 运行环境（含 npm/pnpm） | `node -v` |
| **LLM 端点** | OpenAI 兼容 | 任意 OpenAI-SDK 格式的 LLM API 或本地后端 | — |

### 1. 配置密钥

Teri 不期望在 shell 配置中 `export LLM_API_KEY`。密钥通过 **envctl** 注入（保险库持有，仅子进程环境）。
本地开发可使用被 gitignore 的 `.env`。

```bash
cp .env.example .env   # 仅本地开发

#   LLM_API_KEY=...                # 无密钥本地后端可选
#   LLM_BASE_URL=http://127.0.0.1:11435/v1
#   LLM_MODEL_NAME=OpenThinker3-7B
```

> Teri **无 Zep Cloud 依赖**——时序图谱记忆在进程内原生重新实现，没有 `ZEP_API_KEY`。

### 2. 运行一次模拟（CLI）

```bash
# 通过 envctl（推荐——自动注入保险库密钥）：
envctl run -- teri run \
  --seed ./examples/seed.txt \
  --query "这项政策在 30 天内将如何影响公众情绪？"

# CLI 界面无需任何密钥即可工作：
cargo run --release -- --help
```

`run` 路径会先预检推理后端，再编排 种子 → 图谱 → 智能体 → 模拟 → 报告。

### 3. 启动 API 服务 + Web UI

```bash
# 引擎（REST + SSE），绑定前先预检后端：
envctl run -- teri serve --addr 0.0.0.0:5001

# Web UI（独立开发服务器）：
cd frontend && npm install && npm run dev
```

**服务地址：**
- Web UI：`http://localhost:3000`
- 引擎 API：`http://localhost:5001`

## 🖥️ Web UI

Teri 工作室是一个 Vue 3 单页应用（vue-router、vue-i18n、d3、axios），以向导形式驱动完整工作流：

- **第 1 步 — 图谱构建**：上传种子，观看知识图谱构建（d3 `GraphPanel`）。
- **第 2 步 — 环境搭建**：实体 / 关系审阅，人设 + 智能体配置生成。
- **第 3 步 — 模拟**：实时双平台 tick 流（SSE），上帝视角变量注入。
- **第 4 步 — 报告**：生成的预测报告。
- **第 5 步 — 互动**：与世界中任意智能体对话，或与 ReportAgent 对话。
- **历史数据库**：浏览并重新打开历史运行。
- **国际化**：中文 / English 语言切换。

## ⚙️ 配置

全部配置均通过环境变量（无需配置文件）：

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `LLM_BASE_URL` | `http://127.0.0.1:11435/v1` | OpenAI 兼容的 LLM API 端点 |
| `LLM_API_KEY` | *（无密钥本地可选）* | 托管 LLM 后端的 API Key |
| `LLM_MODEL_NAME` / `LLM_MODEL` | `OpenThinker3-7B` | 补全模型（`LLM_MODEL_NAME` 优先） |
| `EMBED_MODEL` | `all-MiniLM-L6-v2` | 嵌入模型 |
| `DEFAULT_AGENT_COUNT` | `100` | 每次模拟的默认智能体数 |
| `SIM_MAX_TICKS` | `50` | 每次运行的最大 tick 数 |
| `RUST_LOG` | `teri=debug,tower_http=info` | 日志级别 |

## 🏗️ 架构

引擎是一个带类型的五阶段流水线：`seed → graph → agent → sim → report`，通过 CLI（`teri run`）与
REST/SSE 服务（`teri serve`）暴露。模块契约见 [`ARCHITECTURE.md`](./ARCHITECTURE.md)，权威的对齐
校验面见 [`RUNBOOK.md`](./RUNBOOK.md)。

## 🛡️ 后端诚实性守卫

`run` 与 `serve` 都会以 fail-closed 方式预检后端：`GET /models`（身份）与 1-token 补全探针。
列不出模型、或以罐头桩文本回答的后端会被**拒绝**——在罐头文本上模拟出的蜂群是捏造而非预测。该守卫绝不
为了让运行通过而被削弱。

## 📊 状态

Teri 是广义的智能体场景引擎，而非预言机。它能模拟并预测契合其种子数据、本体、人设、动作、记忆与报告
模型的场景。它不证明因果真相；除非另行加入校准回路，报告 `confidence` 属合成的报告元数据而非校准后的
概率。

相对 MiroFish 的对齐情况记录于 [`RUNBOOK.md`](./RUNBOOK.md) §12 及特性对齐账本。**Web UI** 是相对
MiroFish 仍在推进中的主要界面。

## 📄 致谢

Teri 是 **[MiroFish](https://github.com/666ghj/MiroFish)**（作者 BaiFu / 666ghj，AGPL-3.0）的独立
MIT 重新实现——按规格对齐，绝不照抄代码。其仿真设计借鉴了 CAMEL-AI 团队的
**[OASIS](https://github.com/camel-ai/oasis)**。谨此致谢。

## 许可证

MIT
