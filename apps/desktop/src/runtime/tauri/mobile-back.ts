import type { AppSystemBackSubscriber } from "@markra/app/runtime";
import { invokeNative } from "./invoke";

const mobileBackRequestedEvent = "qingyu://mobile-back-requested";

async function handleMobileSystemBack(handler: () => Promise<boolean>) {
  try {
    const authorized = await invokeNative<boolean>("begin_mobile_back");
    if (!authorized) return;

    let consumed = true;
    try {
      consumed = await handler();
    } catch {
      // A failed navigation guard must fail closed so Android does not exit accidentally.
    }

    await invokeNative("complete_mobile_back", { consumed });
  } catch {
    // Native bridge failures keep the live Activity in place and fail closed.
  }
}

export const subscribeToMobileSystemBack: AppSystemBackSubscriber = async (handler) => {
  const handleBack = () => {
    handleMobileSystemBack(handler).catch(() => {});
  };

  window.addEventListener(mobileBackRequestedEvent, handleBack);

  return () => window.removeEventListener(mobileBackRequestedEvent, handleBack);
};
