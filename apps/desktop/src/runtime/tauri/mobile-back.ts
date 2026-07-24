import type { AppSystemBackSubscriber } from "@markra/app/runtime";
import { listenNativeEvent } from "./events";
import { invokeNative } from "./invoke";

const mobileBackRequestedEvent = "qingyu://mobile-back-requested";

export const subscribeToMobileSystemBack: AppSystemBackSubscriber = (handler) => (
  listenNativeEvent(mobileBackRequestedEvent, async () => {
    let consumed = true;
    try {
      consumed = await handler();
    } catch {
      // A failed navigation guard must fail closed so Android does not exit accidentally.
    }

    await invokeNative("complete_mobile_back", { consumed });
  })
);
