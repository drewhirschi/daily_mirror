import { test } from "node:test";
import assert from "node:assert/strict";
import { MirrorApi } from "@daily-mirror/api";

const origin = "https://mirror.example";

function stub(handler: (url: string, options?: RequestInit) => Response) {
  const original = globalThis.fetch;
  globalThis.fetch = (async (url, options) =>
    handler(String(url), options)) as typeof fetch;
  return () => {
    globalThis.fetch = original;
  };
}

test("the household summary carries the signed-in account's role", async () => {
  const restore = stub(() =>
    Response.json({
      id: "house",
      display_name: "Hirschi",
      grid_size: 4,
      self_person_id: "person",
      role: "admin",
      people: [
        {
          id: "person",
          display_name: "Drew",
          role: "admin",
          account: "linked",
          enrollment: {
            enrolled: true,
            enrolled_photos: 5,
            required_photos: 5,
          },
        },
        {
          id: "guest",
          display_name: "Sam",
          role: null,
          account: "none",
          enrollment: {
            enrolled: false,
            enrolled_photos: 0,
            required_photos: 5,
          },
        },
      ],
    }),
  );
  try {
    const household = await new MirrorApi(origin, "token").household();
    assert.equal(household.display_name, "Hirschi");
    assert.equal(household.role, "admin");
    assert.equal(household.people[0].role, "admin");
    assert.equal(household.people[1].role, null);
    // The invite seam: a person with no login of their own.
    assert.equal(household.people[0].account, "linked");
    assert.equal(household.people[1].account, "none");
  } finally {
    restore();
  }
});

test("renaming patches the household and returns the new summary", async () => {
  const calls: { url: string; method?: string; body: any }[] = [];
  const restore = stub((url, options) => {
    calls.push({
      url,
      method: options?.method,
      body: JSON.parse(String(options?.body)),
    });
    return Response.json({
      id: "house",
      display_name: "Hirschi",
      grid_size: 4,
      self_person_id: null,
      role: "admin",
      people: [],
    });
  });
  try {
    const renamed = await new MirrorApi(origin, "token").renameHousehold(
      "Hirschi",
    );
    assert.equal(renamed.display_name, "Hirschi");
    assert.equal(calls[0].url, `${origin}/api/household`);
    assert.equal(calls[0].method, "PATCH");
    assert.deepEqual(calls[0].body, { display_name: "Hirschi" });
  } finally {
    restore();
  }
});

test("a member's rename attempt surfaces the server's 403", async () => {
  const restore = stub(() =>
    Response.json({ error: "Only a household administrator can do that" }, {
      status: 403,
    }),
  );
  try {
    await assert.rejects(
      new MirrorApi(origin, "token").renameHousehold("Nope"),
      (error: any) => error.status === 403,
    );
  } finally {
    restore();
  }
});
