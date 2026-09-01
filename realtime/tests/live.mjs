import { createHmac } from "node:crypto";
import { spawn } from "node:child_process";
import WebSocket from "ws";

const port = 18_799;
const origin = "http://127.0.0.1:3000";
const ticketSecret = "integration-ticket-secret-000000000000000000000000";
const publishToken = "integration-publish-token-00000000000000000000000";
const baseUrl = `http://127.0.0.1:${port}`;
const logs = [];

const wrangler = spawn(
  process.platform === "win32" ? "wrangler.cmd" : "wrangler",
  [
    "dev", "--port", String(port),
    "--var", `TICKET_SECRET:${ticketSecret}`,
    "--var", `PUBLISH_TOKEN:${publishToken}`,
  ],
  { stdio: ["ignore", "pipe", "pipe"] },
);
wrangler.stdout.on("data", (chunk) => logs.push(String(chunk)));
wrangler.stderr.on("data", (chunk) => logs.push(String(chunk)));

try {
  await waitForHealth();
  const householdId = "integration-home";
  const ticket = signTicket({
    household_id: householdId,
    expires_at: Math.floor(Date.now() / 1000) + 60,
    nonce: "integration-nonce",
  });
  const socket = new WebSocket(
    `ws://127.0.0.1:${port}/v1/households/${householdId}/connect?ticket=${ticket}`,
    { origin },
  );
  const messages = [];
  socket.on("message", (data) => messages.push(JSON.parse(String(data))));
  await event(socket, "open");

  const unauthorized = await fetch(`${baseUrl}/v1/households/${householdId}/events`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "photo.created", data: { photo_id: "nope" } }),
  });
  assert(unauthorized.status === 401, `expected unauthorized publisher to get 401, got ${unauthorized.status}`);

  const published = await fetch(`${baseUrl}/v1/households/${householdId}/events`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${publishToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ type: "photo.created", data: { photo_id: "capture-1" } }),
  });
  if (!published.ok) throw new Error(`publisher returned ${published.status}: ${await published.text()}`);
  const receipt = await published.json();
  assert(receipt.delivered === 1, `expected one delivery, got ${JSON.stringify(receipt)}`);
  await until(() => messages.some((message) => message.type === "photo.created"));
  const photoEvent = messages.find((message) => message.type === "photo.created");
  assert(photoEvent.household_id === householdId, "event crossed household boundary");
  assert(photoEvent.sequence >= 1, "event did not receive a durable sequence");
  assert(photoEvent.data.photo_id === "capture-1", "event payload changed");
  socket.close(1000, "test complete");
  console.log("Rust Worker live test passed: ticket, Durable Object, publisher auth, and WebSocket broadcast");
} catch (error) {
  await delay(500);
  console.error(logs.join(""));
  throw error;
} finally {
  wrangler.kill("SIGINT");
  await Promise.race([event(wrangler, "exit"), delay(3_000)]);
  if (wrangler.exitCode === null) wrangler.kill("SIGKILL");
}

function signTicket(claims) {
  const payload = Buffer.from(JSON.stringify(claims)).toString("base64url");
  const signature = createHmac("sha256", ticketSecret).update(payload).digest("base64url");
  return `${payload}.${signature}`;
}

async function waitForHealth() {
  for (let attempt = 0; attempt < 300; attempt += 1) {
    if (wrangler.exitCode !== null) throw new Error(`Wrangler exited with ${wrangler.exitCode}`);
    try {
      const response = await fetch(`${baseUrl}/healthz`);
      if (response.ok) return;
    } catch {}
    await delay(100);
  }
  throw new Error("Wrangler did not become healthy within 30 seconds");
}

async function until(predicate) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (predicate()) return;
    await delay(50);
  }
  throw new Error("Timed out waiting for realtime message");
}

function event(emitter, name) {
  return new Promise((resolve, reject) => {
    emitter.once(name, resolve);
    emitter.once("error", reject);
  });
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
