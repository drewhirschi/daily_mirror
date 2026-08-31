import type { ReactNode } from "react";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <div className="app-shell">
      <header className="topbar">
        <a href="/" className="brand">Daily Mirror</a>
        <nav className="topbar-actions" aria-label="Account navigation">
          <a href="/account">Account</a>
          <span className="status-mark" title="Archive online" aria-label="Archive online" />
        </nav>
      </header>
      {children}
    </div>
  );
}
