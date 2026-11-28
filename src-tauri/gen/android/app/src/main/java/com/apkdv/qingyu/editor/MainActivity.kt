package com.apkdv.qingyu.editor

import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import android.os.SystemClock
import android.view.View
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.annotation.Keep
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.Insets
import androidx.core.graphics.drawable.IconCompat
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

private const val SHORTCUT_ACTION = "com.apkdv.qingyu.editor.SHORTCUT"
private const val SHORTCUT_EXTRA = "shortcut"
private val SHORTCUT_KINDS = setOf("memo", "voice", "note")

class MainActivity : TauriActivity() {
  @Keep
  private external fun initTlsVerifier(context: Context)

  private var editorWebView: WebView? = null
  private var appDark: Boolean? = null
  private var appBackground: Int? = null
  private var verticalInsets: String? = null
  private var insetReply: JavaScriptReplyProxy? = null
  private var splashReady = false
  // 桌面图标长按菜单带进来的入口，等页面说就绪后再交给网页处理。
  private var pendingShortcut: String? = null
  private var pageReady = false
  private val webMessageListener = WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)
  private val webHandlesInsets = webMessageListener && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
  private val webOrigins = setOf("http://tauri.localhost")
  private val appearanceMessage = Regex("(light|dark)(?::(#[0-9a-fA-F]{6}))?")

  // 返回键先问页面：有菜单、弹窗或搜索时只收起这一层；页面说没处理，再交给 WryActivity 的 WebView 后退。
  private val backCallback = object : OnBackPressedCallback(true) {
    override fun handleOnBackPressed() {
      val webView = editorWebView ?: return fallBack()
      webView.evaluateJavascript("(() => { try { return window.qingyuBack?.() === true } catch (_) { return false } })()") { handled ->
        if (handled != "true") fallBack()
      }
    }

    private fun fallBack() {
      isEnabled = false
      onBackPressedDispatcher.onBackPressed()
      isEnabled = true
    }
  }

  private fun applyAppearance() {
    val dark = appDark ?: (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES)
    val bars = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT) { dark }
    enableEdgeToEdge(bars, bars)
    window.decorView.setBackgroundColor(appBackground ?: if (dark) Color.BLACK else Color.WHITE)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    val splash = installSplashScreen()
    System.loadLibrary("miaoyan_editor_lib")
    initTlsVerifier(applicationContext)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    if (webMessageListener) {
      val splashDeadline = SystemClock.uptimeMillis() + 4_000L
      splash.setKeepOnScreenCondition { !splashReady && SystemClock.uptimeMillis() < splashDeadline }
    }
    applyAppearance()
    publishShortcuts()
    if (savedInstanceState == null) takeShortcut(intent)
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val systemTypes = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      val bars = insets.getInsets(systemTypes)
      val top = insets.getInsetsIgnoringVisibility(systemTypes).top
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      val reply = insetReply
      // The keyboard still resizes the WebView. The page handles any remaining bottom inset.
      val bottom = maxOf(0, bars.bottom - ime.bottom)
      val bottomPadding = if (reply == null) maxOf(bars.bottom, ime.bottom) else ime.bottom
      view.setPadding(bars.left, if (reply == null) top else 0, bars.right, bottomPadding)
      val nextInsets = "$top,$bottom"
      if (verticalInsets != nextInsets) {
        verticalInsets = nextInsets
        reply?.postMessage(nextInsets)
      }
      val visible = insets.isVisible(WindowInsetsCompat.Type.ime())
      editorWebView?.evaluateJavascript("window.dispatchEvent(new CustomEvent('native-keyboard',{detail:$visible}))", null)
      // Keep notifying WebView when the keyboard hides, without applying native padding twice.
      WindowInsetsCompat.Builder(insets)
        .setInsets(systemTypes or WindowInsetsCompat.Type.ime(), Insets.NONE)
        .build()
    }
    ViewCompat.requestApplyInsets(content)
  }

  override fun onWebViewCreate(webView: WebView) {
    editorWebView = webView
    // 在 WryActivity 的回调之后注册，所以先收到返回键。
    onBackPressedDispatcher.addCallback(this, backCallback)
    if (webHandlesInsets) installInsetBridge(webView)
    if (webMessageListener) {
      WebViewCompat.addWebMessageListener(webView, "nativeSplash", webOrigins) { view, message, _, isMainFrame, _ ->
        if (!isMainFrame || message.data != "ready") return@addWebMessageListener
        pageReady = true
        deliverShortcut()
        if (splashReady) return@addWebMessageListener
        view.postVisualStateCallback(0L, object : WebView.VisualStateCallback() {
          override fun onComplete(requestId: Long) { splashReady = true }
        })
      }
      WebViewCompat.addWebMessageListener(webView, "nativeAppearance", webOrigins) { _, message, _, isMainFrame, _ ->
        if (!isMainFrame) return@addWebMessageListener
        val (theme, color) = message.data?.let(appearanceMessage::matchEntire)?.destructured ?: return@addWebMessageListener
        val dark = theme == "dark"
        val background = color.takeIf { it.isNotEmpty() }?.let(Color::parseColor)
        if (dark == appDark && background == appBackground) return@addWebMessageListener
        appDark = dark
        appBackground = background
        applyAppearance()
      }
    }
    ViewCompat.requestApplyInsets(findViewById(android.R.id.content))
    deliverShortcut()
  }

  private fun installInsetBridge(webView: WebView) {
    insetReply = null
    WebViewCompat.addWebMessageListener(webView, "nativeInsets", webOrigins) { _, message, _, isMainFrame, reply ->
      if (!isMainFrame || message.data != "ready") return@addWebMessageListener
      val firstReply = insetReply == null
      insetReply = reply
      if (firstReply) ViewCompat.requestApplyInsets(findViewById<View>(android.R.id.content))
      verticalInsets?.let { reply.postMessage(it) }
    }
    // Every document requests the current value, including reloads without a new Android insets event.
    val script = """
      (() => {
        if (window !== window.top) return;
        let insets;
        const apply = () => {
          const root = document.documentElement;
          if (!root) return false;
          root.toggleAttribute('data-android-inset-pending', !insets);
          if (insets) {
            const [top, bottom] = insets;
            root.style.setProperty('--safe-area-top', (top / devicePixelRatio) + 'px');
            root.style.setProperty('--safe-area-bottom', (bottom / devicePixelRatio) + 'px');
          }
          return true;
        };
        if (!apply()) {
          const observer = new MutationObserver(() => { if (apply()) observer.disconnect(); });
          observer.observe(document, { childList: true });
        }
        window.nativeInsets.onmessage = event => {
          insets = event.data.split(',').map(Number);
          apply();
        };
        window.nativeInsets.postMessage('ready');
      })();
    """.trimIndent()
    WebViewCompat.addDocumentStartJavaScript(webView, script, webOrigins)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    if (takeShortcut(intent)) deliverShortcut()
  }

  // 动态注册，不写在 shortcuts.xml 里：静态入口要写死包名，debug 包带 .debug 后缀会对不上。
  // 每次启动都重设一遍，切换系统语言后标题也会跟着更新。
  private fun publishShortcuts() {
    val items = listOf(
      Triple("memo", R.string.shortcut_memo, R.drawable.ic_shortcut_memo),
      Triple("voice", R.string.shortcut_voice, R.drawable.ic_shortcut_voice),
      Triple("note", R.string.shortcut_note, R.drawable.ic_shortcut_note),
    ).map { (kind, title, icon) ->
      ShortcutInfoCompat.Builder(this, kind)
        .setShortLabel(getString(title))
        .setIcon(IconCompat.createWithResource(this, icon))
        .setIntent(Intent(this, MainActivity::class.java).setAction(SHORTCUT_ACTION).putExtra(SHORTCUT_EXTRA, kind))
        .build()
    }
    runCatching { ShortcutManagerCompat.setDynamicShortcuts(this, items) }
  }

  private fun takeShortcut(intent: Intent?): Boolean {
    if (intent?.action != SHORTCUT_ACTION) return false
    val kind = intent.getStringExtra(SHORTCUT_EXTRA)?.takeIf { it in SHORTCUT_KINDS } ?: return false
    pendingShortcut = kind
    // 同一个 Intent 在旋转屏幕等重建时还会再来一次，用过就清掉。
    intent.action = Intent.ACTION_MAIN
    return true
  }

  private fun deliverShortcut() {
    val kind = pendingShortcut ?: return
    val webView = editorWebView ?: return
    // 老版本 WebView 没有就绪消息，页面加载完的时间猜不准，只能稍等再发。
    if (!pageReady && webMessageListener) return
    pendingShortcut = null
    val run = { webView.evaluateJavascript("window.qingyuShortcut?.('$kind')", null) }
    if (pageReady) run() else webView.postDelayed(run, 1_500L)
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    applyAppearance()
  }
}
