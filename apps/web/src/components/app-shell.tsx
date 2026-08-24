import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import {
  ChatCircleDots,
  Desktop,
  GearSix,
  Moon,
  Pulse,
  Sun,
  TreeStructure,
  WarningCircle,
  WifiHigh,
  WifiSlash
} from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { fetchLocalStatus } from "../lib/api";
import { LeaveMark } from "./leave-mark";

const navigation = [
  { to: "/", label: "Hosts", icon: Desktop },
  { to: "/sessions", label: "Sessions", icon: ChatCircleDots },
  { to: "/workspaces", label: "Workspaces", icon: TreeStructure },
  { to: "/activity", label: "Activity", icon: Pulse },
  { to: "/settings", label: "Settings", icon: GearSix }
] as const;

function useTheme() {
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    const saved = localStorage.getItem("leave-theme");
    if (saved === "light" || saved === "dark") return saved;
    return matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  });
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("leave-theme", theme);
  }, [theme]);
  return { theme, setTheme };
}

export function AppShell() {
  const { theme, setTheme } = useTheme();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const setup = pathname === "/setup";
  const host = useQuery({
    queryKey: ["local-status"],
    queryFn: ({ signal }) => fetchLocalStatus(signal),
    retry: 2,
    enabled: !setup,
    refetchInterval: 5_000
  });
  const hostOnline = host.isSuccess;
  const agentReady = host.data?.agent.state === "ready";
  const title = navigation.find((item) =>
    item.to === "/" ? pathname === "/" : pathname.startsWith(item.to)
  )?.label ?? "Leave";

  useEffect(() => {
    document.getElementById("main-content")?.focus({ preventScroll: true });
  }, [pathname]);

  if (setup) {
    return (
      <div className="setup-shell">
        <a className="skip-link" href="#main-content">Skip to setup</a>
        <header className="setup-topbar">
          <div className="brand" aria-label="Leave setup">
            <span className="brand-mark"><LeaveMark size={25} /></span>
            <span className="brand-word">leave</span>
            <span className="alpha-label">setup</span>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            aria-label={`Use ${theme === "dark" ? "light" : "dark"} theme`}
          >
            {theme === "dark" ? <Sun aria-hidden="true" size={18} /> : <Moon aria-hidden="true" size={18} />}
          </button>
        </header>
        <main id="main-content" className="setup-main" tabIndex={-1}>
          <div className="route-stage" key={pathname}><Outlet /></div>
        </main>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">Skip to content</a>
      <aside className="sidebar">
        <Link className="brand" to="/" aria-label="Leave home">
          <span className="brand-mark"><LeaveMark size={25} /></span>
          <span className="brand-word">leave</span>
          <span className="alpha-label">lab</span>
        </Link>
        <nav className="side-nav" aria-label="Primary navigation">
          {navigation.map(({ to, label, icon: Icon }) => (
            <Link key={to} to={to} className="nav-item" activeProps={{ className: "nav-item active" }}>
              <Icon aria-hidden="true" size={19} weight="regular" />
              <span>{label}</span>
            </Link>
          ))}
        </nav>
        <div className="security-card">
          <div className="security-card-title"><span className="security-marker" />{host.data?.mode === "tailnet" ? "Private away access" : "Local workspace"}</div>
          <p>{host.data?.mode === "tailnet" ? "Reachable through Tailscale only by the host owner's identity." : "Devin and repository content stay on this computer."}</p>
          <Link to="/settings">View gate status</Link>
        </div>
        <button
          className="theme-toggle"
          type="button"
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          aria-label={`Use ${theme === "dark" ? "light" : "dark"} theme`}
        >
          {theme === "dark" ? <Sun aria-hidden="true" size={18} weight="regular" /> : <Moon aria-hidden="true" size={18} weight="regular" />}
          <span>{theme === "dark" ? "Light theme" : "Dark theme"}</span>
        </button>
      </aside>

      <header className="mobile-header">
        <Link className="brand compact-brand" to="/" aria-label="Leave home">
          <span className="brand-mark"><LeaveMark size={23} /></span>
          <span>{title}</span>
        </Link>
        <div className={`connection-indicator ${agentReady ? "is-online" : "is-offline"}`}>
          {agentReady
            ? <><WifiHigh aria-hidden="true" size={14} weight="regular" /> Devin ready</>
            : <><WifiSlash aria-hidden="true" size={14} weight="regular" /> {hostOnline ? "Agent offline" : "Host offline"}</>}
        </div>
      </header>

      {!agentReady && (
        <div className="offline-banner" role={host.error || host.data?.agent.state === "error" ? "alert" : "status"}>
          {host.isPending
            ? <><WifiHigh aria-hidden="true" size={16} weight="regular" /> Connecting to the Leave host…</>
            : host.error
              ? <><WifiSlash aria-hidden="true" size={16} weight="regular" /> Leave host is unavailable. Start it with <code>leave connect .</code>.</>
              : <><WarningCircle aria-hidden="true" size={16} weight="regular" /> {host.data?.agent.detail ?? "Devin ACP is unavailable. Run leave doctor."}</>}
        </div>
      )}

      <main id="main-content" className="main-content" tabIndex={-1}>
        <div className="route-stage" key={pathname}><Outlet /></div>
      </main>

      <nav className="bottom-nav" aria-label="Primary navigation">
        {navigation.map(({ to, label, icon: Icon }) => (
          <Link key={to} to={to} className="bottom-nav-item" activeProps={{ className: "bottom-nav-item active" }}>
            <Icon aria-hidden="true" size={20} weight="regular" />
            <span>{label}</span>
          </Link>
        ))}
      </nav>
    </div>
  );
}
