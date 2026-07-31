import {
  useEffect,
  useId,
  useRef,
  useState,
  type FormEvent,
  type InputHTMLAttributes,
  type RefObject,
} from "react";
import {
  CircleAlert,
  Clock3,
  Eye,
  EyeOff,
  Server,
  TriangleAlert,
} from "lucide-react";
import type { AppRuntime } from "@markra/app/runtime";

import type {
  ServerWebBootstrapOwner,
  ServerWebBootstrapSnapshot,
  ServerWebPromptError,
} from "./server-bootstrap";
import type { ServerStartupLanguage } from "./server-startup-language";

export interface ServerStartupShellProps {
  readonly language?: ServerStartupLanguage;
  readonly owner: ServerWebBootstrapOwner;
  readonly serverAddress?: string;
  readonly snapshot: ServerWebBootstrapSnapshot;
  readonly transport?: "HTTP" | "HTTPS";
}

export function ServerStartupShell({
  language = "en",
  owner,
  serverAddress = "QingYu Server",
  snapshot,
  transport = "HTTP",
}: ServerStartupShellProps) {
  useEffect(() => {
    const markPointerFocus = () => {
      document.body.dataset.serverFocusOrigin = "pointer";
    };
    const markKeyboardFocus = (event: KeyboardEvent) => {
      if (event.key === "Tab") document.body.dataset.serverFocusOrigin = "keyboard";
    };
    document.addEventListener("pointerdown", markPointerFocus, { capture: true });
    document.addEventListener("keydown", markKeyboardFocus, { capture: true });
    return () => {
      document.removeEventListener("pointerdown", markPointerFocus, { capture: true });
      document.removeEventListener("keydown", markKeyboardFocus, { capture: true });
      delete document.body.dataset.serverFocusOrigin;
    };
  }, []);

  if (snapshot.phase === "closed") return null;

  const copy = startupCopy[language];
  const titleId = `server-startup-title-${snapshot.phase}`;

  return (
    <main aria-labelledby={titleId} className="server-startup">
      <aside className="server-startup__context" aria-label={copy.serverInformation}>
        <div className="server-startup__brand">
          <span className="server-startup__brand-mark" aria-hidden="true">
            <img alt="" height="32" src="/favicon.png" width="32" />
          </span>
          <span>
            {copy.brandName}
            <span className="server-startup__brand-subtitle">QingYu Server</span>
          </span>
        </div>

        <div className="server-startup__slogan" aria-hidden="true">
          <p>
            <span>{copy.slogan[0]}</span>
            <span>{copy.slogan[1]}</span>
          </p>
        </div>

        <div className="server-startup__meta" data-connected={snapshot.phase !== "failed"}>
          <div className="server-startup__address">
            <span className="server-startup__status-dot" aria-hidden="true" />
            <span title={serverAddress}>{serverAddress}</span>
          </div>
          <span className="server-startup__transport">{transport}</span>
        </div>
      </aside>

      <section className="server-startup__panel">
        <div className="server-startup__content" aria-live="polite">
          {snapshot.phase === "login" ? (
            <LoginForm
              copy={copy}
              error={snapshot.error}
              owner={owner}
              titleId={titleId}
            />
          ) : null}
          {snapshot.phase === "initialize" ? (
            <InitializeForm
              copy={copy}
              error={snapshot.error}
              owner={owner}
              titleId={titleId}
            />
          ) : null}
          {snapshot.phase === "failed" ? (
            <UnavailableState copy={copy} owner={owner} titleId={titleId} />
          ) : null}
          {snapshot.phase === "checking" || snapshot.phase === "starting" ? (
            <LoadingState copy={copy} phase={snapshot.phase} titleId={titleId} />
          ) : null}
        </div>
      </section>
    </main>
  );
}

function LoginForm({
  copy,
  error,
  owner,
  titleId,
}: {
  readonly copy: StartupCopy;
  readonly error: ServerWebPromptError | null;
  readonly owner: ServerWebBootstrapOwner;
  readonly titleId: string;
}) {
  const initialRetrySeconds = error?.kind === "rate-limited"
    ? error.retryAfterSeconds
    : 0;
  const [retrySeconds, setRetrySeconds] = useState(initialRetrySeconds);

  useEffect(() => {
    setRetrySeconds(error?.kind === "rate-limited" ? error.retryAfterSeconds : 0);
  }, [error]);

  useEffect(() => {
    if (retrySeconds <= 0) return undefined;
    const timer = window.setTimeout(() => {
      setRetrySeconds((current) => Math.max(0, current - 1));
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [retrySeconds]);

  const visibleError = error?.kind === "rate-limited" && retrySeconds <= 0
    ? null
    : error;
  const submitLogin = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const password = readSecret(new FormData(form), "password");
    form.reset();
    owner.login({ password }).catch(() => undefined);
  };

  return (
    <section className="server-startup__state-pane">
      <header className="server-startup__header server-startup__header--compact">
        <h1 id={titleId}>{copy.loginTitle}</h1>
      </header>
      <form className="server-startup__form" onSubmit={submitLogin}>
        <PromptError copy={copy} error={visibleError} retrySeconds={retrySeconds} />
        <SecretField
          autoComplete="current-password"
          helper={copy.loginHelper}
          invalid={visibleError?.kind === "invalid-credentials"}
          label={copy.serverPassword}
          name="password"
          required
          revealLabel={copy.showPassword}
          concealLabel={copy.hidePassword}
        />
        <button
          className="server-startup__primary-action"
          disabled={retrySeconds > 0}
          type="submit"
        >
          {copy.signIn}
        </button>
      </form>
    </section>
  );
}

function InitializeForm({
  copy,
  error,
  owner,
  titleId,
}: {
  readonly copy: StartupCopy;
  readonly error: ServerWebPromptError | null;
  readonly owner: ServerWebBootstrapOwner;
  readonly titleId: string;
}) {
  const [confirmationError, setConfirmationError] = useState(false);
  const confirmInput = useRef<HTMLInputElement | null>(null);
  const submitInitialization = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const fields = new FormData(form);
    const initializationToken = readSecret(fields, "initializationToken");
    const password = readSecret(fields, "password");
    const confirmation = readSecret(fields, "confirmPassword");
    if (password !== confirmation) {
      setConfirmationError(true);
      confirmInput.current?.focus({ preventScroll: true });
      return;
    }

    setConfirmationError(false);
    form.reset();
    owner.initialize({ initializationToken, password }).catch(() => undefined);
  };
  const revalidateConfirmation = (event: FormEvent<HTMLInputElement>) => {
    if (!confirmationError) return;
    const form = event.currentTarget.form;
    if (form === null) return;
    const fields = new FormData(form);
    setConfirmationError(
      readSecret(fields, "password") !== readSecret(fields, "confirmPassword"),
    );
  };

  return (
    <section className="server-startup__state-pane">
      <header className="server-startup__header">
        <h1 id={titleId}>{copy.initializeTitle}</h1>
        <p>{copy.initializeIntroduction}</p>
      </header>
      <form className="server-startup__form" onSubmit={submitInitialization}>
        <PromptError copy={copy} error={error} initializing />
        <SecretField
          autoComplete="off"
          helper={copy.tokenHelper}
          invalid={error?.kind === "invalid-credentials"}
          label={copy.initializationToken}
          name="initializationToken"
          required
          revealLabel={copy.showToken}
          concealLabel={copy.hideToken}
        />
        <SecretField
          autoComplete="new-password"
          helper={copy.passwordHelper}
          invalid={error?.kind === "invalid-credentials"}
          label={copy.ownerPassword}
          minLength={12}
          name="password"
          required
          revealLabel={copy.showPassword}
          concealLabel={copy.hidePassword}
        />
        <SecretField
          autoComplete="new-password"
          helper={confirmationError ? copy.passwordMismatch : copy.confirmationHelper}
          helperTone={confirmationError ? "error" : undefined}
          inputRef={confirmInput}
          invalid={confirmationError}
          label={copy.confirmPassword}
          name="confirmPassword"
          onInput={revalidateConfirmation}
          required
          revealLabel={copy.showConfirmation}
          concealLabel={copy.hideConfirmation}
        />
        <button className="server-startup__primary-action" type="submit">
          {copy.completeSetup}
        </button>
      </form>
    </section>
  );
}

function SecretField({
  concealLabel,
  helper,
  helperTone,
  inputRef,
  invalid = false,
  label,
  revealLabel,
  ...inputProps
}: InputHTMLAttributes<HTMLInputElement> & {
  readonly concealLabel: string;
  readonly helper: string;
  readonly helperTone?: "error";
  readonly inputRef?: RefObject<HTMLInputElement | null>;
  readonly invalid?: boolean;
  readonly label: string;
  readonly revealLabel: string;
}) {
  const generatedId = useId();
  const localInputRef = useRef<HTMLInputElement | null>(null);
  const [revealed, setRevealed] = useState(false);
  const inputId = `server-secret-${generatedId}`;
  const helperId = `${inputId}-message`;
  const activeRef = inputRef ?? localInputRef;

  return (
    <div className="server-startup__field">
      <label className="server-startup__field-label" htmlFor={inputId}>
        {label}
      </label>
      <span className="server-startup__field-control">
        <input
          {...inputProps}
          aria-describedby={helperId}
          aria-invalid={invalid || undefined}
          className="server-startup__field-input"
          id={inputId}
          ref={activeRef}
          type={revealed ? "text" : "password"}
        />
        <button
          aria-label={revealed ? concealLabel : revealLabel}
          aria-pressed={revealed}
          className="server-startup__field-action"
          onClick={() => {
            setRevealed((current) => !current);
            activeRef.current?.focus({ preventScroll: true });
          }}
          type="button"
        >
          {revealed ? <EyeOff aria-hidden="true" /> : <Eye aria-hidden="true" />}
        </button>
      </span>
      <span
        className="server-startup__field-message"
        data-tone={helperTone}
        id={helperId}
      >
        {helper}
      </span>
    </div>
  );
}

function PromptError({
  copy,
  error,
  initializing = false,
  retrySeconds,
}: {
  readonly copy: StartupCopy;
  readonly error: ServerWebPromptError | null;
  readonly initializing?: boolean;
  readonly retrySeconds?: number;
}) {
  if (error === null) return null;
  if (error.kind === "rate-limited") {
    return (
      <div className="server-startup__alert" data-tone="warning" role="alert">
        <Clock3 aria-hidden="true" />
        <p>{copy.rateLimited(retrySeconds ?? error.retryAfterSeconds)}</p>
      </div>
    );
  }
  if (error.kind === "invalid-credentials") {
    return (
      <div className="server-startup__alert" role="alert">
        <CircleAlert aria-hidden="true" />
        <p>{initializing ? copy.initializationRejected : copy.credentialsRejected}</p>
      </div>
    );
  }
  return (
    <div className="server-startup__alert" role="alert">
      <CircleAlert aria-hidden="true" />
      <p>{copy.serverUnavailableInline}</p>
    </div>
  );
}

function LoadingState({
  copy,
  phase,
  titleId,
}: {
  readonly copy: StartupCopy;
  readonly phase: "checking" | "starting";
  readonly titleId: string;
}) {
  const checking = phase === "checking";
  return (
    <section
      aria-busy="true"
      className="server-startup__state-pane server-startup__status-pane"
    >
      <div className="server-startup__status-symbol" aria-hidden="true">
        <Server />
      </div>
      <header className="server-startup__header">
        <h1 id={titleId}>{checking ? copy.checkingTitle : copy.startingTitle}</h1>
        <p>{checking ? copy.checkingDescription : copy.startingDescription}</p>
      </header>
      <div
        aria-label={checking ? copy.checkingProgress : copy.startingProgress}
        className="server-startup__progress"
        role="progressbar"
      />
    </section>
  );
}

function UnavailableState({
  copy,
  owner,
  titleId,
}: {
  readonly copy: StartupCopy;
  readonly owner: ServerWebBootstrapOwner;
  readonly titleId: string;
}) {
  return (
    <section className="server-startup__state-pane server-startup__status-pane">
      <div className="server-startup__status-symbol" data-tone="error" aria-hidden="true">
        <TriangleAlert />
      </div>
      <header className="server-startup__header">
        <h1 id={titleId}>{copy.unavailableTitle}</h1>
        <p>{copy.unavailableDescription}</p>
      </header>
      <button
        className="server-startup__secondary-action"
        onClick={() => owner.retry().catch(() => undefined)}
        type="button"
      >
        {copy.retry}
      </button>
    </section>
  );
}

export interface StartServerWebApplicationOptions {
  readonly configureRuntime: (runtime: AppRuntime) => unknown;
  readonly createRuntime: (
    kernel: Extract<ServerWebBootstrapSnapshot, { phase: "ready" }>["result"]["kernel"],
  ) => AppRuntime;
  readonly owner: ServerWebBootstrapOwner;
  readonly renderApp: () => unknown;
  readonly renderStartup: (
    snapshot: ServerWebBootstrapSnapshot,
    owner: ServerWebBootstrapOwner,
  ) => unknown;
}

export function startServerWebApplication({
  configureRuntime,
  createRuntime,
  owner,
  renderApp,
  renderStartup,
}: StartServerWebApplicationOptions) {
  let stopped = false;
  let mountedKernel: object | undefined;

  const unsubscribe = owner.subscribe((snapshot) => {
    if (stopped) return undefined;
    if (snapshot.phase !== "ready") {
      mountedKernel = undefined;
      renderStartup(snapshot, owner);
      return undefined;
    }
    if (mountedKernel === snapshot.result.kernel) return undefined;
    mountedKernel = snapshot.result.kernel;
    const runtime = createRuntime(snapshot.result.kernel);
    configureRuntime(runtime);
    renderApp();
    return undefined;
  });

  owner.start().catch(() => undefined);

  return () => {
    if (stopped) return undefined;
    stopped = true;
    unsubscribe();
    owner.close();
    return undefined;
  };
}

function readSecret(form: FormData, name: string) {
  const value = form.get(name);
  return typeof value === "string" ? value : "";
}

type StartupCopy = {
  brandName: string;
  checkingDescription: string;
  checkingProgress: string;
  checkingTitle: string;
  completeSetup: string;
  confirmPassword: string;
  confirmationHelper: string;
  credentialsRejected: string;
  hideConfirmation: string;
  hidePassword: string;
  hideToken: string;
  initializationRejected: string;
  initializationToken: string;
  initializeIntroduction: string;
  initializeTitle: string;
  loginHelper: string;
  loginTitle: string;
  ownerPassword: string;
  passwordHelper: string;
  passwordMismatch: string;
  rateLimited: (seconds: number) => string;
  retry: string;
  serverInformation: string;
  serverPassword: string;
  serverUnavailableInline: string;
  showConfirmation: string;
  showPassword: string;
  showToken: string;
  signIn: string;
  slogan: readonly [string, string];
  startingDescription: string;
  startingProgress: string;
  startingTitle: string;
  tokenHelper: string;
  unavailableDescription: string;
  unavailableTitle: string;
};

const startupCopy: Record<ServerStartupLanguage, StartupCopy> = {
  en: {
    brandName: "QingYu",
    checkingDescription: "Checking authentication and workspace availability.",
    checkingProgress: "Checking the server",
    checkingTitle: "Connecting to the server",
    completeSetup: "Complete setup",
    confirmPassword: "Confirm password",
    confirmationHelper: "Enter the password again to prevent typing errors.",
    credentialsRejected: "The password was not accepted. Check it and sign in again.",
    hideConfirmation: "Hide confirmation password",
    hidePassword: "Hide password",
    hideToken: "Hide initialization token",
    initializationRejected: "The initialization token or password was not accepted. Check both and try again.",
    initializationToken: "One-time initialization token",
    initializeIntroduction: "Use the one-time token generated during deployment, then set the owner password.",
    initializeTitle: "Set up this server",
    loginHelper: "This server has one owner account.",
    loginTitle: "Welcome back",
    ownerPassword: "Owner password",
    passwordHelper: "Use at least 12 characters and a password unique to this server.",
    passwordMismatch: "The passwords do not match. Enter the same password again.",
    rateLimited: (seconds) => `Too many attempts. Try again in ${seconds} seconds.`,
    retry: "Reconnect",
    serverInformation: "Server information",
    serverPassword: "Server password",
    serverUnavailableInline: "The server is temporarily unavailable. Try again shortly.",
    showConfirmation: "Show confirmation password",
    showPassword: "Show password",
    showToken: "Show initialization token",
    signIn: "Sign in",
    slogan: ["A clear desk,", "every word softly spoken."],
    startingDescription: "Preparing the workspace and secure session.",
    startingProgress: "Starting the server workspace",
    startingTitle: "Starting QingYu Server",
    tokenHelper: "The token expires immediately after successful verification.",
    unavailableDescription: "The current address did not respond. Confirm the container is running, check the network, and reconnect.",
    unavailableTitle: "Unable to connect",
  },
  "zh-CN": {
    brandName: "轻语",
    checkingDescription: "检查身份验证状态和工作区可用性。",
    checkingProgress: "正在检查服务器",
    checkingTitle: "正在连接服务器",
    completeSetup: "完成设置",
    confirmPassword: "确认密码",
    confirmationHelper: "再次输入密码以避免误输。",
    credentialsRejected: "密码未通过验证。请检查后再次登录。",
    hideConfirmation: "隐藏确认密码",
    hidePassword: "隐藏密码",
    hideToken: "隐藏初始化令牌",
    initializationRejected: "初始化令牌或密码未通过验证。请检查后再次设置。",
    initializationToken: "一次性初始化令牌",
    initializeIntroduction: "使用部署时生成的一次性令牌，并为所有者设置密码。",
    initializeTitle: "设置这台服务器",
    loginHelper: "此服务器只有一个所有者账户。",
    loginTitle: "欢迎回来",
    ownerPassword: "所有者密码",
    passwordHelper: "至少 12 个字符。请使用独立密码。",
    passwordMismatch: "两次密码不一致。请再次输入相同的密码。",
    rateLimited: (seconds) => `尝试次数过多。${seconds} 秒后可再次登录。`,
    retry: "重新连接",
    serverInformation: "服务器信息",
    serverPassword: "服务器密码",
    serverUnavailableInline: "服务器暂时不可用，请稍后再试。",
    showConfirmation: "显示确认密码",
    showPassword: "显示密码",
    showToken: "显示初始化令牌",
    signIn: "登录",
    slogan: ["明窗净几，", "字字轻语。"],
    startingDescription: "正在准备工作区和安全会话。",
    startingProgress: "正在启动服务器工作区",
    startingTitle: "正在启动服务器",
    tokenHelper: "令牌验证成功后立即失效。",
    unavailableDescription: "当前地址没有响应。请确认容器正在运行，并检查网络后重试。",
    unavailableTitle: "无法连接服务器",
  },
};
