import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { WelcomeScreen, type WelcomeScreenProps } from "./WelcomeScreen";

function callbacks(): Pick<
  WelcomeScreenProps,
  | "error"
  | "onChooseDesktopRoot"
  | "onCreateMobileRoot"
  | "onDeferDesktopSetup"
  | "onOpenExternalFile"
  | "onRetry"
> {
  return {
    error: null,
    onChooseDesktopRoot: vi.fn(async () => undefined),
    onCreateMobileRoot: vi.fn(async () => undefined),
    onDeferDesktopSetup: vi.fn(async () => undefined),
    onOpenExternalFile: vi.fn(async () => undefined),
    onRetry: vi.fn(async () => undefined)
  };
}

function restoreFromCloud() {
  return vi.fn(async () => undefined);
}

function setViewport(width: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  window.dispatchEvent(new Event("resize"));
}

describe("WelcomeScreen", () => {
  it("renders the approved true-mobile action set", () => {
    const props = callbacks();

    render(
      <WelcomeScreen
        formFactor="mobile"
        language="zh-CN"
        status="needs-onboarding"
        {...props}
      />
    );

    expect(screen.getByText("明窗净几，字字轻语。")).toBeVisible();
    expect(screen.getByRole("button", { name: "创建并开始" })).toBeVisible();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.queryByText(/第一步|稍后再说/u)).not.toBeInTheDocument();
  });

  it("keeps the complete notebook action set at a narrow desktop viewport", () => {
    setViewport(375);
    const props = callbacks();
    const onRestoreFromCloud = restoreFromCloud();

    render(
      <WelcomeScreen
        formFactor="desktop"
        language="zh-CN"
        status="needs-onboarding"
        onRestoreFromCloud={onRestoreFromCloud}
        {...props}
      />
    );

    expect(screen.getByRole("button", { name: "选择本地笔记目录…" })).toBeVisible();
    expect(screen.getByRole("button", { name: "从云端恢复" })).toBeVisible();
    expect(screen.getByRole("button", { name: "打开单独文件" })).toBeVisible();
    expect(screen.getByRole("button", { name: "稍后再说" })).toBeVisible();
    expect(screen.getAllByRole("button")).toHaveLength(4);
    expect(screen.queryByText(/第一步|First step/u)).not.toBeInTheDocument();
  });

  it("omits step copy from the wide desktop welcome", () => {
    setViewport(1280);

    render(
      <WelcomeScreen
        formFactor="desktop"
        language="en"
        status="needs-onboarding"
        {...callbacks()}
      />
    );

    expect(screen.getByRole("heading", { name: "Choose your notes folder" })).toBeVisible();
    expect(screen.getByText("QingYu")).toBeVisible();
    expect(screen.queryByText(/第一步|First step/u)).not.toBeInTheDocument();
  });

  it("keeps the approved QingYu promise prominent beside the desktop setup task", () => {
    render(
      <WelcomeScreen
        formFactor="desktop"
        language="zh-CN"
        status="needs-onboarding"
        onRestoreFromCloud={restoreFromCloud()}
        {...callbacks()}
      />
    );

    expect(screen.getByText("明窗净几，字字轻语。")).toBeVisible();
    expect(screen.getByRole("heading", { name: "选择你的笔记目录" })).toBeVisible();
    expect(screen.queryByText(/步骤|第一步/u)).not.toBeInTheDocument();
  });

  it("routes each onboarding action through its matching callback", async () => {
    const props = callbacks();
    const onRestoreFromCloud = restoreFromCloud();
    const { rerender } = render(
      <WelcomeScreen
        formFactor="desktop"
        language="zh-CN"
        status="needs-onboarding"
        onRestoreFromCloud={onRestoreFromCloud}
        {...props}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "选择本地笔记目录…" }));
    fireEvent.click(screen.getByRole("button", { name: "从云端恢复" }));
    fireEvent.click(screen.getByRole("button", { name: "稍后再说" }));
    fireEvent.click(screen.getByRole("button", { name: "打开单独文件" }));

    rerender(
      <WelcomeScreen
        formFactor="mobile"
        language="zh-CN"
        status="needs-onboarding"
        {...props}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "创建并开始" }));

    await waitFor(() => {
      expect(props.onChooseDesktopRoot).toHaveBeenCalledOnce();
      expect(onRestoreFromCloud).toHaveBeenCalledOnce();
      expect(props.onCreateMobileRoot).toHaveBeenCalledOnce();
      expect(props.onDeferDesktopSetup).toHaveBeenCalledOnce();
      expect(props.onOpenExternalFile).toHaveBeenCalledOnce();
    });
  });

  it("offers retry and replacement directory actions for desktop recovery", async () => {
    const props = callbacks();

    render(
      <WelcomeScreen
        formFactor="desktop"
        language="zh-CN"
        status="recovery"
        {...props}
      />
    );

    expect(screen.getByRole("heading", { name: "找不到原来的笔记目录" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    fireEvent.click(screen.getByRole("button", { name: "选择其他目录…" }));

    await waitFor(() => {
      expect(props.onRetry).toHaveBeenCalledOnce();
      expect(props.onChooseDesktopRoot).toHaveBeenCalledOnce();
    });
  });

  it("keeps the mobile error state to one bottom retry action", async () => {
    const props = callbacks();

    render(
      <WelcomeScreen
        formFactor="mobile"
        language="zh-CN"
        status="error"
        {...props}
        error="应用数据目录不可用。"
      />
    );

    expect(screen.getByRole("heading", { name: "暂时无法准备笔记目录" })).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("应用数据目录不可用。");
    const retryButton = screen.getByRole("button", { name: "重试" });
    expect(screen.getAllByRole("button")).toEqual([retryButton]);
    expect(screen.queryByRole("button", { name: "选择目录…" })).not.toBeInTheDocument();

    fireEvent.click(retryButton);
    await waitFor(() => expect(props.onRetry).toHaveBeenCalledOnce());
  });

  it("announces loading without exposing premature actions", () => {
    render(
      <WelcomeScreen
        formFactor="desktop"
        language="zh-CN"
        status="loading"
        {...callbacks()}
      />
    );

    expect(screen.getByRole("status")).toHaveTextContent("正在准备轻语…");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
