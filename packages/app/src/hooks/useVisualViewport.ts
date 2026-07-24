import { useEffect, useState } from "react";

export type VisualViewportMetrics = {
  keyboardInset: number;
  layoutHeight: number;
  offsetTop: number;
  visualHeight: number;
};

function readVisualViewport(): VisualViewportMetrics {
  const layoutHeight = window.innerHeight;
  const viewport = window.visualViewport;
  if (!viewport) {
    return {
      keyboardInset: 0,
      layoutHeight,
      offsetTop: 0,
      visualHeight: layoutHeight
    };
  }

  return {
    keyboardInset: Math.max(0, layoutHeight - viewport.height - viewport.offsetTop),
    layoutHeight,
    offsetTop: viewport.offsetTop,
    visualHeight: viewport.height
  };
}

function sameMetrics(left: VisualViewportMetrics, right: VisualViewportMetrics) {
  return left.keyboardInset === right.keyboardInset
    && left.layoutHeight === right.layoutHeight
    && left.offsetTop === right.offsetTop
    && left.visualHeight === right.visualHeight;
}

export function useVisualViewport() {
  const [metrics, setMetrics] = useState(readVisualViewport);

  useEffect(() => {
    const viewport = window.visualViewport;
    const updateMetrics = () => {
      const nextMetrics = readVisualViewport();
      setMetrics((currentMetrics) => sameMetrics(currentMetrics, nextMetrics) ? currentMetrics : nextMetrics);
    };

    window.addEventListener("resize", updateMetrics);
    viewport?.addEventListener("resize", updateMetrics);
    viewport?.addEventListener("scroll", updateMetrics);
    updateMetrics();

    return () => {
      window.removeEventListener("resize", updateMetrics);
      viewport?.removeEventListener("resize", updateMetrics);
      viewport?.removeEventListener("scroll", updateMetrics);
    };
  }, []);

  return metrics;
}
