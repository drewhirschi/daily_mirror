import type { ReactNode } from "react";
import { PwaMetadata } from "../components/pwa";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <>
      <PwaMetadata />
      <div className="app-shell">
        <header className="topbar">
          <a href="/" className="brand">Daily Mirror</a>
          <nav className="topbar-actions" aria-label="Application navigation">
            <a href="/">Gallery</a>
            <a href="/admin">Faces</a>
            <a href="/account">Account</a>
            <span className="status-mark" title="Archive online" aria-label="Archive online" />
          </nav>
        </header>
        {children}
      </div>
    </>
  );
}
