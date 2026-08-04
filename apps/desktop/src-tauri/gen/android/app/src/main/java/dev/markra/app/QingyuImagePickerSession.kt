package dev.markra.app

interface QingyuImagePickerInvoke {
  fun resolveImagePickerUris(uris: List<String>)

  fun rejectImagePicker(message: String)
}

class QingyuImagePickerSettlingInvoke(
  private val delegate: QingyuImagePickerInvoke,
  private val wakeEventLoop: () -> Unit
) : QingyuImagePickerInvoke {
  override fun resolveImagePickerUris(uris: List<String>) {
    settle { delegate.resolveImagePickerUris(uris) }
  }

  override fun rejectImagePicker(message: String) {
    settle { delegate.rejectImagePicker(message) }
  }

  private fun settle(action: () -> Unit) {
    try {
      action()
    } finally {
      try {
        wakeEventLoop()
      } catch (_: Exception) {
      }
    }
  }
}

class QingyuImagePickerSession {
  private var pendingInvoke: QingyuImagePickerInvoke? = null

  @Synchronized
  fun begin(invoke: QingyuImagePickerInvoke): Boolean {
    if (pendingInvoke != null) {
      invoke.rejectImagePicker("Image picker already running")
      return false
    }
    pendingInvoke = invoke
    return true
  }

  @Synchronized
  fun complete(resultCode: Int, uris: List<String>): Boolean {
    val invoke = pendingInvoke ?: return false
    pendingInvoke = null
    when (resultCode) {
      RESULT_OK -> invoke.resolveImagePickerUris(uris)
      RESULT_CANCELED -> invoke.resolveImagePickerUris(emptyList())
      else -> invoke.rejectImagePicker("Failed to pick files")
    }
    return true
  }

  @Synchronized
  fun reject(message: String): Boolean {
    val invoke = pendingInvoke ?: return false
    pendingInvoke = null
    invoke.rejectImagePicker(message)
    return true
  }

  @Synchronized
  fun hasPendingInvoke(): Boolean = pendingInvoke != null

  private companion object {
    const val RESULT_OK = -1
    const val RESULT_CANCELED = 0
  }
}
