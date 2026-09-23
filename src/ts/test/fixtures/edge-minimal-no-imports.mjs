export default async function middleware({
  request,
  fetch,
  auth,
  rateLimit,
  cache,
  telemetry,
  context
}) {
  const identity = await auth.verify();
  const key = `user:${identity.userId ?? "anonymous"}`;
  const allowed = await rateLimit.allow(key, 100, 20);

  if (!allowed) {
    return {
      kind: "respond",
      response: new Response("rate limited", { status: 429 })
    };
  }

  let profile = await cache.get(key);
  if (!profile) {
    const response = await fetch("https://identity.example.test/profile");
    profile = new TextEncoder().encode(await response.text());
    await cache.set(key, profile, 30_000);
  }

  request.setHeader("x-ores-user-id", identity.userId ?? "anonymous");
  await telemetry.event("edge_minimal.authorized", {
    requestId: context.requestId,
    cacheBytes: profile.byteLength
  });

  return { kind: "continue" };
}
