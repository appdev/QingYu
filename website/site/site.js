(() => {
  const REPO = "appdev/QingYu";
  const MIRROR = "https://ghfast.top/";
  const zh = document.documentElement.lang.startsWith("zh");

  // 滚动后给顶栏加一条细线
  const nav = document.querySelector(".nav");
  const onScroll = () => nav.classList.toggle("scrolled", window.scrollY > 8);
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  // 区块进入视口时淡入
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
    }
  }, { rootMargin: "0px 0px -8% 0px" });
  document.querySelectorAll(".reveal").forEach((el) => io.observe(el));

  // 安装包文件名规则，和 release 工作流的命名保持一致
  const patterns = {
    "mac-arm": /_darwin_aarch64\.dmg$/,
    "mac-x64": /_darwin_x64\.dmg$/,
    "win": /_windows_x64-setup\.exe$/,
    "linux-appimage": /_linux_amd64\.AppImage$/,
    "linux-deb": /_linux_amd64\.deb$/,
    "android": /_android_arm64\.apk$/,
  };

  const setLinks = (key, url) => {
    document.querySelectorAll(`[data-asset="${key}"]`).forEach((a) => { a.href = url; });
    document.querySelectorAll(`[data-mirror="${key}"]`).forEach((a) => { a.href = MIRROR + url; });
  };

  // 页面里写死的是发布时的版本，这里再去 GitHub 取一次最新版，取不到就保持原样
  fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } })
    .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
    .then((release) => {
      for (const [key, re] of Object.entries(patterns)) {
        const asset = release.assets.find((a) => re.test(a.name));
        if (asset) setLinks(key, asset.browser_download_url);
      }
      const tag = release.tag_name;
      const date = new Date(release.published_at).toLocaleDateString(zh ? "zh-CN" : "en-US", { year: "numeric", month: "long", day: "numeric" });
      document.querySelectorAll("[data-version]").forEach((el) => { el.textContent = tag; });
      document.querySelectorAll("[data-release-date]").forEach((el) => { el.textContent = date; });
      document.querySelectorAll("[data-release-notes]").forEach((el) => { el.href = release.html_url; });
      pickPrimary();
    })
    .catch(() => {});

  // 首屏主按钮按访客系统换成对应安装包
  const ua = navigator.userAgent;
  const platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  let os = "";
  if (/android/i.test(ua)) os = "android";
  else if (/iphone|ipad|ipod/i.test(ua) || (platform === "MacIntel" && navigator.maxTouchPoints > 1)) os = "ios";
  else if (/mac/i.test(platform)) os = "mac";
  else if (/win/i.test(platform)) os = "win";
  else if (/linux/i.test(platform)) os = "linux";

  const labels = zh
    ? { mac: "下载 Mac 版", win: "下载 Windows 版", linux: "下载 Linux 版", android: "下载 Android 版" }
    : { mac: "Download for Mac", win: "Download for Windows", linux: "Download for Linux", android: "Download for Android" };

  let macKey = "mac-arm";
  const pickPrimary = () => {
    const btn = document.querySelector("[data-primary-download]");
    if (!btn || !labels[os]) return;
    const key = { mac: macKey, win: "win", linux: "linux-appimage", android: "android" }[os];
    const source = document.querySelector(`[data-asset="${key}"]`);
    if (!source) return;
    btn.href = source.href;
    btn.querySelector("span").textContent = labels[os];
    btn.removeAttribute("data-scroll");
  };

  if (os) {
    const card = document.querySelector(`.dl[data-os="${os === "ios" ? "" : os}"]`);
    if (card) card.classList.add("current");
  }

  // Chromium 能拿到 CPU 架构，Intel Mac 给 x64 包；其他浏览器默认 Apple 芯片
  if (os === "mac" && navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
    navigator.userAgentData.getHighEntropyValues(["architecture"]).then((v) => {
      if (v.architecture === "x86") macKey = "mac-x64";
      pickPrimary();
    }).catch(() => {});
  }
  pickPrimary();
})();
