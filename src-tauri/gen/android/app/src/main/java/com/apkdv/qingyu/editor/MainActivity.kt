package com.apkdv.qingyu.editor

import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.annotation.Keep
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

class MainActivity : TauriActivity() {
  @Keep
  private external fun initTlsVerifier(context: Context)

  private var editorWebView: WebView? = null
  private var appDark: Boolean? = null
  private var appBackground: Int? = null
  private var verticalInsets: String? = null
  private var insetReply: JavaScriptReplyProxy? = null
  private val webHandlesInsets = WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT) &&
    WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)
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
    System.loadLibrary("miaoyan_editor_lib")
    initTlsVerifier(applicationContext)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    applyAppearance()
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
    if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
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

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    applyAppearance()
  }
}
