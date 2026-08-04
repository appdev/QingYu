package dev.markra.app

import java.io.File
import org.junit.Assert.assertFalse
import org.junit.Test

class QingyuImagePickerBoundaryTest {
  @Test
  fun mainActivityDoesNotSettlePickerByInvokingTauriFromWebViewJavascript() {
    val source = sourceFile("src/main/java/dev/markra/app/MainActivity.kt").readText()

    assertFalse(
      "Picker settlement must not depend on a WebView-to-Tauri invoke wake that can be queued behind the same unsettled command",
      source.contains("wake_mobile_picker_event_loop")
    )
    assertFalse(
      "Picker settlement should not wrap Invoke.resolve in the old JavaScript wake strategy",
      source.contains("QingyuImagePickerSettlingInvoke")
    )
  }

  private fun sourceFile(relativePath: String): File {
    var directory = File(System.getProperty("user.dir") ?: ".")
    repeat(5) {
      val candidate = File(directory, relativePath)
      if (candidate.isFile) return candidate
      directory.parentFile?.let { parent -> directory = parent } ?: return@repeat
    }
    throw AssertionError("Could not locate source file $relativePath")
  }
}
