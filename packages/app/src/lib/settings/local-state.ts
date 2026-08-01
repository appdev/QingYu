import { getAppRuntime, type RuntimeStore } from "../../runtime";

const localStateStorePath = "local-state.json";
const localStateSchemaVersionKey = "schemaVersion";
const localStateSchemaVersion = 2;
const welcomeDocumentSeenKey = "welcomeDocumentSeen";

function localStore() {
  return getAppRuntime().settings.loadStore(localStateStorePath, {
    autoSave: false,
    defaults: {}
  });
}

async function saveLocalStore(store: RuntimeStore) {
  await store.set(localStateSchemaVersionKey, localStateSchemaVersion);
  await store.save();
}

export async function consumeWelcomeDocumentState() {
  const store = await localStore();
  const hasSeenWelcomeDocument = await store.get<boolean>(welcomeDocumentSeenKey);
  if (hasSeenWelcomeDocument) return false;

  await store.set(welcomeDocumentSeenKey, true);
  await saveLocalStore(store);
  return true;
}

export async function resetWelcomeDocumentState() {
  const store = await localStore();
  await store.delete(welcomeDocumentSeenKey);
  await saveLocalStore(store);
}
