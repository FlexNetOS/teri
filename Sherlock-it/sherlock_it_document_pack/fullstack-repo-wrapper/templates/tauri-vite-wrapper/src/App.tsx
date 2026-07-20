import React from "react";
import { repoConfig } from "./bridge";

export default function App() {
  return (
    <main style={{ fontFamily: "Inter, sans-serif", margin: "0 auto", maxWidth: 960, padding: 24 }}>
      <h1>{repoConfig.productName} Desktop Wrapper</h1>
      <p>
        This shell wraps the verified application surface for <strong>{repoConfig.productName}</strong>.
        Point the wrapper at built web assets or a trusted runtime endpoint after the repository itself passes verification.
      </p>

      <section>
        <h2>Repository</h2>
        <table>
          <tbody>
            <tr><td><strong>Repo URL</strong></td><td>{repoConfig.repoUrl}</td></tr>
            <tr><td><strong>App ID</strong></td><td>{repoConfig.appId}</td></tr>
            <tr><td><strong>Target URL</strong></td><td>{repoConfig.webTargetUrl}</td></tr>
          </tbody>
        </table>
      </section>

      <section>
        <h2>Next Steps</h2>
        <ol>
          <li>Build and verify the source repository first.</li>
          <li>Replace this page with embedded build assets or a trusted webview target.</li>
          <li>Record wrapper evidence in the mission report before packaging.</li>
        </ol>
      </section>
    </main>
  );
}
