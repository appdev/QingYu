package dev.markra.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class QingyuImagePickerSessionTest {
  @Test
  fun completingAfterActivityReplacementResolvesOriginalInvoke() {
    val session = QingyuImagePickerSession()
    val invoke = RecordingImagePickerInvoke()

    assertTrue(session.begin(invoke))
    assertTrue(session.complete(RESULT_OK, listOf("content://media/picker/image/1")))

    assertEquals(listOf(listOf("content://media/picker/image/1")), invoke.resolved)
    assertEquals(emptyList<String>(), invoke.rejected)
    assertFalse(session.hasPendingInvoke())
  }

  @Test
  fun resultWithoutPendingInvokeIsReportedInsteadOfSilentlyCompleting() {
    val session = QingyuImagePickerSession()

    assertFalse(session.complete(RESULT_OK, listOf("content://media/picker/image/1")))
  }

  @Test
  fun cancellationSettlesWithAnEmptySelection() {
    val session = QingyuImagePickerSession()
    val invoke = RecordingImagePickerInvoke()

    assertTrue(session.begin(invoke))
    assertTrue(session.complete(RESULT_CANCELED, listOf("content://media/picker/image/1")))

    assertEquals(listOf(emptyList<String>()), invoke.resolved)
    assertEquals(emptyList<String>(), invoke.rejected)
    assertFalse(session.hasPendingInvoke())
  }

  @Test
  fun secondPickerIsRejectedWhileFirstIsPending() {
    val session = QingyuImagePickerSession()
    val first = RecordingImagePickerInvoke()
    val second = RecordingImagePickerInvoke()

    assertTrue(session.begin(first))
    assertFalse(session.begin(second))

    assertEquals(listOf("Image picker already running"), second.rejected)
    assertTrue(session.complete(RESULT_OK, listOf("content://media/picker/image/1")))
    assertEquals(listOf(listOf("content://media/picker/image/1")), first.resolved)
  }

  private class RecordingImagePickerInvoke : QingyuImagePickerInvoke {
    val resolved = mutableListOf<List<String>>()
    val rejected = mutableListOf<String>()

    override fun resolveImagePickerUris(uris: List<String>) {
      resolved.add(uris)
    }

    override fun rejectImagePicker(message: String) {
      rejected.add(message)
    }
  }

  private companion object {
    const val RESULT_OK = -1
    const val RESULT_CANCELED = 0
  }
}
