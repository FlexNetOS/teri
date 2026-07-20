import React from "react";

const repoUrl = "__REPO_URL__";
const webTargetUrl = import.meta.env.VITE_WRAPPED_APP_URL || "__WEB_TARGET_URL__";

export default function App() {
  return (
    <main style={{ fontFamily: "Inter, sans-serif", margin: "0 auto", maxWidth: 1100, padding: 24 }}>
      <h1>__PRODUCT_NAME__ Web Wrapper</h1>
      <p>
        This PWA shell is the honest fallback for browser and mobile-browser delivery when a native mobile path is not yet proven.
      </p>
      <p><strong>Repository:</strong> {repoUrl}</p>
      <p><strong>Wrapped target:</strong> {webTargetUrl}</p>
      <iframe
        title="Wrapped application"
        src={webTargetUrl}
        style={{ width: "100%", minHeight: 700, border: "1px solid #d1d5db", borderRadius: 12 }}
      />
    </main>
  );
}
