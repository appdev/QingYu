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
  fun successfulResultWakesEventLoopAfterResolvingInvoke() {
    val events = mutableListOf<String>()
    val session = QingyuImagePickerSession()
    val invoke = QingyuImagePickerSettlingInvoke(
      RecordingImagePickerInvoke(events),
      { events.add("wake") }
    )

    assertTrue(session.begin(invoke))
    assertTrue(session.complete(RESULT_OK, listOf("content://media/picker/image/1")))

    assertEquals(listOf("resolve:content://media/picker/image/1", "wake"), events)
    assertFalse(session.hasPendingInvoke())
  }

  @Test
  fun cancelledResultWakesEventLoopAfterResolvingEmptySelection() {
    val events = mutableListOf<String>()
    val session = QingyuImagePickerSession()
    val invoke = QingyuImagePickerSettlingInvoke(
      RecordingImagePickerInvoke(events),
      { events.add("wake") }
    )

    assertTrue(session.begin(invoke))
    assertTrue(session.complete(RESULT_CANCELED, listOf("content://media/picker/image/1")))

    assertEquals(listOf("resolve:", "wake"), events)
    assertFalse(session.hasPendingInvoke())
  }

  @Test
  fun rejectedResultWakesEventLoopAfterRejectingInvoke() {
    val events = mutableListOf<String>()
    val session = QingyuImagePickerSession()
    val invoke = QingyuImagePickerSettlingInvoke(
      RecordingImagePickerInvoke(events),
      { events.add("wake") }
    )

    assertTrue(session.begin(invoke))
    assertTrue(session.complete(RESULT_DENIED, listOf("content://media/picker/image/1")))

    assertEquals(listOf("reject:Failed to pick files", "wake"), events)
    assertFalse(session.hasPendingInvoke())
  }

  @Test
  fun eventLoopWakeFailureDoesNotReplacePickerResult() {
    val events = mutableListOf<String>()
    val session = QingyuImagePickerSession()
    val invoke = QingyuImagePickerSettlingInvoke(
      RecordingImagePickerInvoke(events),
      { throw IllegalStateException("wake failed") }
    )

    assertTrue(session.begin(invoke))
    assertTrue(session.complete(RESULT_OK, listOf("content://media/picker/image/1")))

    assertEquals(listOf("resolve:content://media/picker/image/1"), events)
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

  @Test
  fun rejectedSecondPickerWakesWithoutDisturbingFirstPendingInvoke() {
    val firstEvents = mutableListOf<String>()
    val secondEvents = mutableListOf<String>()
    val session = QingyuImagePickerSession()
    val first = QingyuImagePickerSettlingInvoke(
      RecordingImagePickerInvoke(firstEvents),
      { firstEvents.add("wake") }
    )
    val second = QingyuImagePickerSettlingInvoke(
      RecordingImagePickerInvoke(secondEvents),
      { secondEvents.add("wake") }
    )

    assertTrue(session.begin(first))
    assertFalse(session.begin(second))

    assertEquals(emptyList<String>(), firstEvents)
    assertEquals(listOf("reject:Image picker already running", "wake"), secondEvents)

    assertTrue(session.complete(RESULT_OK, listOf("content://media/picker/image/1")))
    assertEquals(listOf("resolve:content://media/picker/image/1", "wake"), firstEvents)
  }

  private class RecordingImagePickerInvoke(
    private val events: MutableList<String> = mutableListOf()
  ) : QingyuImagePickerInvoke {
    val resolved = mutableListOf<List<String>>()
    val rejected = mutableListOf<String>()

    override fun resolveImagePickerUris(uris: List<String>) {
      resolved.add(uris)
      events.add("resolve:${uris.joinToString(",")}")
    }

    override fun rejectImagePicker(message: String) {
      rejected.add(message)
      events.add("reject:$message")
    }
  }

  private companion object {
    const val RESULT_OK = -1
    const val RESULT_CANCELED = 0
    const val RESULT_DENIED = 7
  }
}
