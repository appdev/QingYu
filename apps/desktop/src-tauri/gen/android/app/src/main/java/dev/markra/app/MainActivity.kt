package dev.markra.app

import android.os.Bundle
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var appWebView: WebView? = null
  private val systemBackCallback = object : OnBackPressedCallback(true) {
    override fun handleOnBackPressed() {
      appWebView?.evaluateJavascript(MOBILE_BACK_SCRIPT, null)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    onBackPressedDispatcher.addCallback(this, systemBackCallback)
  }

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    super.onWebViewCreate(webView)
  }

  override fun onDestroy() {
    appWebView = null
    super.onDestroy()
  }

  private companion object {
    const val MOBILE_BACK_SCRIPT =
      "window.dispatchEvent(new Event('qingyu://mobile-back-requested'))"
  }
}
