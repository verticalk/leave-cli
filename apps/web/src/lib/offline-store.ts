const DATABASE_NAME = "leave-encrypted-cache";
const STORE_NAME = "envelopes";

interface StoredEnvelope {
  id: string;
  workspaceId: string;
  receivedAt: number;
  ciphertext: ArrayBuffer;
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) {
        const store = database.createObjectStore(STORE_NAME, { keyPath: "id" });
        store.createIndex("workspace", "workspaceId");
        store.createIndex("receivedAt", "receivedAt");
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("IndexedDB open failed"));
  });
}

export async function putEncryptedEnvelope(
  id: string,
  workspaceId: string,
  ciphertext: Uint8Array
): Promise<void> {
  if (ciphertext.byteLength === 0) throw new Error("Refusing to cache an empty envelope");
  const database = await openDatabase();
  const copy = new Uint8Array(ciphertext.byteLength);
  copy.set(ciphertext);
  await new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, "readwrite");
    const record: StoredEnvelope = {
      id,
      workspaceId,
      receivedAt: Date.now(),
      ciphertext: copy.buffer
    };
    transaction.objectStore(STORE_NAME).put(record);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error ?? new Error("Encrypted cache write failed"));
  });
  database.close();
}

export async function clearEncryptedCache(): Promise<void> {
  const database = await openDatabase();
  await new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, "readwrite");
    transaction.objectStore(STORE_NAME).clear();
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error ?? new Error("Encrypted cache clear failed"));
  });
  database.close();
}
