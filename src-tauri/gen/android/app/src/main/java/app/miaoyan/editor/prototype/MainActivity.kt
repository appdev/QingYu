package app.miaoyan.editor.prototype

import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

class MainActivity : TauriActivity() {
  private var editorWebView: WebView? = null
  private var appDark: Boolean? = null
  private var appBackground: Int? = null
  private val appearanceMessage = Regex("(light|dark)(?::(#[0-9a-fA-F]{6}))?")

  private fun applyAppearance() {
    val dark = appDark ?: (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES)
    val bars = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT) { dark }
    enableEdgeToEdge(bars, bars)
    window.decorView.setBackgroundColor(appBackground ?: if (dark) Color.BLACK else Color.WHITE)
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    applyAppearance()
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val systemTypes = WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      val bars = insets.getInsets(systemTypes)
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
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
    if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
      WebViewCompat.addWebMessageListener(webView, "nativeAppearance", setOf("http://tauri.localhost")) { _, message, _, isMainFrame, _ ->
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

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    applyAppearance()
  }
}
