import { act, renderHook } from "@testing-library/react";
import { useVisualViewport } from "./useVisualViewport";

type MutableVisualViewport = {
  viewport: VisualViewport;
  emit: (type: "resize" | "scroll") => unknown;
  setMetrics: (metrics: { height?: number; offsetTop?: number }) => unknown;
};

const originalInnerHeight = Object.getOwnPropertyDescriptor(window, "innerHeight");
const originalVisualViewport = Object.getOwnPropertyDescriptor(window, "visualViewport");

function setInnerHeight(innerHeight: number) {
  Object.defineProperty(window, "innerHeight", {
    configurable: true,
    value: innerHeight
  });
}

function installVisualViewport(initialHeight: number, initialOffsetTop: number): MutableVisualViewport {
  let height = initialHeight;
  let offsetTop = initialOffsetTop;
  const listeners = new Map<string, Set<EventListenerOrEventListenerObject>>();
  const viewport = {
    get height() {
      return height;
    },
    get offsetTop() {
      return offsetTop;
    },
    width: 390,
    offsetLeft: 0,
    pageLeft: 0,
    pageTop: 0,
    scale: 1,
    onresize: null,
    onscroll: null,
    addEventListener: vi.fn((type: string, listener: EventListenerOrEventListenerObject) => {
      const eventListeners = listeners.get(type) ?? new Set<EventListenerOrEventListenerObject>();
      eventListeners.add(listener);
      listeners.set(type, eventListeners);
    }),
    removeEventListener: vi.fn((type: string, listener: EventListenerOrEventListenerObject) => {
      listeners.get(type)?.delete(listener);
    }),
    dispatchEvent: vi.fn()
  } as unknown as VisualViewport;

  Object.defineProperty(window, "visualViewport", {
    configurable: true,
    value: viewport
  });

  return {
    viewport,
    emit(type) {
      const event = new Event(type);
      listeners.get(type)?.forEach((listener) => {
        if (typeof listener === "function") listener(event);
        else listener.handleEvent(event);
      });
    },
    setMetrics(metrics) {
      height = metrics.height ?? height;
      offsetTop = metrics.offsetTop ?? offsetTop;
    }
  };
}

describe("useVisualViewport", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    if (originalInnerHeight) Object.defineProperty(window, "innerHeight", originalInnerHeight);
    if (originalVisualViewport) Object.defineProperty(window, "visualViewport", originalVisualViewport);
    else Reflect.deleteProperty(window, "visualViewport");
  });

  it("updates keyboard inset for visual viewport resize and scroll changes", () => {
    setInnerHeight(800);
    const visualViewport = installVisualViewport(600, 20);
    const { result } = renderHook(() => useVisualViewport());

    expect(result.current).toEqual({
      keyboardInset: 180,
      layoutHeight: 800,
      offsetTop: 20,
      visualHeight: 600
    });

    act(() => {
      visualViewport.setMetrics({ height: 500 });
      visualViewport.emit("resize");
    });
    expect(result.current.keyboardInset).toBe(280);
    expect(result.current.visualHeight).toBe(500);

    act(() => {
      visualViewport.setMetrics({ offsetTop: 35 });
      visualViewport.emit("scroll");
    });
    expect(result.current.keyboardInset).toBe(265);
    expect(result.current.offsetTop).toBe(35);
  });

  it("falls back to the layout viewport when visualViewport is missing", () => {
    setInnerHeight(744);
    Reflect.deleteProperty(window, "visualViewport");

    const { result } = renderHook(() => useVisualViewport());

    expect(result.current).toEqual({
      keyboardInset: 0,
      layoutHeight: 744,
      offsetTop: 0,
      visualHeight: 744
    });

    act(() => {
      setInnerHeight(680);
      window.dispatchEvent(new Event("resize"));
    });
    expect(result.current).toEqual({
      keyboardInset: 0,
      layoutHeight: 680,
      offsetTop: 0,
      visualHeight: 680
    });
  });

  it("deduplicates unchanged viewport events", () => {
    setInnerHeight(800);
    const visualViewport = installVisualViewport(600, 20);
    const { result } = renderHook(() => useVisualViewport());
    const initialMetrics = result.current;

    act(() => visualViewport.emit("resize"));
    act(() => visualViewport.emit("scroll"));

    expect(result.current).toBe(initialMetrics);
  });

  it("removes both visual viewport listeners on unmount", () => {
    setInnerHeight(800);
    const visualViewport = installVisualViewport(600, 20);
    const windowAddEventListener = vi.spyOn(window, "addEventListener");
    const windowRemoveEventListener = vi.spyOn(window, "removeEventListener");
    const { unmount } = renderHook(() => useVisualViewport());
    const windowResizeListener = windowAddEventListener.mock.calls
      .find(([type]) => type === "resize")?.[1];
    const resizeListener = vi.mocked(visualViewport.viewport.addEventListener).mock.calls
      .find(([type]) => type === "resize")?.[1];
    const scrollListener = vi.mocked(visualViewport.viewport.addEventListener).mock.calls
      .find(([type]) => type === "scroll")?.[1];

    expect(windowResizeListener).toBeDefined();
    expect(resizeListener).toBeDefined();
    expect(scrollListener).toBeDefined();
    unmount();

    expect(windowRemoveEventListener).toHaveBeenCalledWith("resize", windowResizeListener);
    expect(visualViewport.viewport.removeEventListener).toHaveBeenCalledWith("resize", resizeListener);
    expect(visualViewport.viewport.removeEventListener).toHaveBeenCalledWith("scroll", scrollListener);
  });
});
