import { useEffect, useState } from "react";

export type LeaveTheme = "dark" | "light";

function readTheme(): LeaveTheme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function useObservedTheme(): LeaveTheme {
  const [theme, setTheme] = useState<LeaveTheme>(readTheme);

  useEffect(() => {
    const observer = new MutationObserver(() => setTheme(readTheme()));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  return theme;
}
