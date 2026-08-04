package dev.markra.app

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import app.tauri.Logger
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject

class MainActivity : TauriActivity() {
  private var appWebView: WebView? = null
  private var pendingImagePickerInvoke: Invoke? = null
  private lateinit var imagePickerLauncher: ActivityResultLauncher<Intent>
  private val systemBackCallback = object : OnBackPressedCallback(true) {
    override fun handleOnBackPressed() {
      appWebView?.evaluateJavascript(MOBILE_BACK_SCRIPT, null)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    imagePickerLauncher = registerForActivityResult(
      ActivityResultContracts.StartActivityForResult()
    ) { result ->
      completeImagePicker(result)
    }
    super.onCreate(savedInstanceState)
    onBackPressedDispatcher.addCallback(this, systemBackCallback)
  }

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    super.onWebViewCreate(webView)
  }

  override fun onDestroy() {
    appWebView = null
    pendingImagePickerInvoke?.reject("Image picker cancelled")
    pendingImagePickerInvoke = null
    super.onDestroy()
  }

  fun launchImagePicker(invoke: Invoke, title: String?) {
    if (pendingImagePickerInvoke != null) {
      invoke.reject("Image picker already running")
      return
    }

    val intent = Intent(Intent.ACTION_GET_CONTENT)
    intent.addCategory(Intent.CATEGORY_OPENABLE)
    intent.type = "image/*"
    intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
    title?.trim()?.takeIf { it.isNotEmpty() }?.let {
      intent.putExtra(Intent.EXTRA_TITLE, it)
    }

    try {
      pendingImagePickerInvoke = invoke
      imagePickerLauncher.launch(intent)
    } catch (error: Exception) {
      pendingImagePickerInvoke = null
      val message = error.message ?: "Failed to pick files"
      Logger.error(message)
      invoke.reject(message)
    }
  }

  private fun completeImagePicker(result: ActivityResult) {
    val invoke = pendingImagePickerInvoke ?: return
    pendingImagePickerInvoke = null

    try {
      when (result.resultCode) {
        Activity.RESULT_OK -> invoke.resolve(imagePickerResponse(result.data))
        Activity.RESULT_CANCELED -> invoke.resolve(imagePickerResponse(null))
        else -> invoke.reject("Failed to pick files")
      }
    } catch (error: Exception) {
      val message = error.message ?: "Failed to read image pick result"
      Logger.error(message)
      invoke.reject(message)
    }
  }

  private fun imagePickerResponse(data: Intent?): JSObject {
    val uris = mutableListOf<String>()
    val clipData = data?.clipData
    if (clipData != null) {
      for (index in 0 until clipData.itemCount) {
        clipData.getItemAt(index).uri?.let { uri: Uri -> uris.add(uri.toString()) }
      }
    } else {
      data?.data?.let { uris.add(it.toString()) }
    }

    val response = JSObject()
    response.put("uris", JSArray.from(uris.toTypedArray()))
    return response
  }

  private companion object {
    const val MOBILE_BACK_SCRIPT =
      "window.dispatchEvent(new Event('qingyu://mobile-back-requested'))"
  }
}
