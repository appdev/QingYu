import { useEffect, useState } from "react";
import { getAppRuntime, type AppFormFactor } from "../runtime";

export const compactViewportQuery = "(max-width: 720px)";

export type CompactMode = {
  compact: boolean;
  formFactor: AppFormFactor;
  trueMobile: boolean;
};

export function useCompactMode(): CompactMode {
  const [formFactor] = useState(() => getAppRuntime().platform.resolveFormFactor());
  const [mediaQuery] = useState(() => window.matchMedia(compactViewportQuery));
  const [viewportCompact, setViewportCompact] = useState(mediaQuery.matches);

  useEffect(() => {
    const handleChange = (event: MediaQueryListEvent) => {
      setViewportCompact(event.matches);
    };

    mediaQuery.addEventListener("change", handleChange);

    return () => {
      mediaQuery.removeEventListener("change", handleChange);
    };
  }, [mediaQuery]);

  const trueMobile = formFactor === "mobile";

  return {
    compact: trueMobile || viewportCompact,
    formFactor,
    trueMobile
  };
}
