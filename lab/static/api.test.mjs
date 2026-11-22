// 실행: node --test lab/static/api.test.mjs   (Node 20+, 서버 불필요 — 가짜 fetch로 계약만 검증한다)
import assert from "node:assert/strict";
import test from "node:test";
import { api, ApiError, fieldMessages } from "./api.js";

/** 상태 코드와 JSON 본문으로 가짜 fetch를 만든다. 호출 인자를 기록한다. */
function fakeFetch(status, json, calls = []) {
  const impl = async (url, init) => {
    calls.push({ url, init });
    return { status, ok: status >= 200 && status < 300, text: async () => (json === undefined ? "" : JSON.stringify(json)) };
  };
  impl.calls = calls;
  return impl;
}

test("성공 응답은 { data, meta } 를 그대로 돌려준다", async () => {
  const f = fakeFetch(200, { data: [{ id: 1 }], meta: { page: 1, total: 1 } });
  const res = await api("/api/posts", { fetchImpl: f });
  assert.deepEqual(res.data, [{ id: 1 }]);
  assert.equal(res.meta.total, 1);
});

test("본문이 있으면 JSON으로 보내고 토큰은 Bearer 헤더로", async () => {
  const f = fakeFetch(201, { data: { id: 7 } });
  await api("/api/posts", { method: "POST", body: { title: "t" }, token: "abc", fetchImpl: f });
  const { init } = f.calls[0];
  assert.equal(init.headers["Content-Type"], "application/json");
  assert.equal(init.headers.Authorization, "Bearer abc");
  assert.equal(init.body, '{"title":"t"}');
});

test("검증 실패(400)는 code·fields·requestId를 가진 ApiError", async () => {
  const f = fakeFetch(400, { error: { code: "VALIDATION_ERROR", message: "입력값을 확인하세요.", fields: { title: ["1~100자"] }, requestId: "r-1" } });
  await assert.rejects(api("/api/posts", { method: "POST", body: {}, fetchImpl: f }), (e) => {
    assert.ok(e instanceof ApiError);
    assert.equal(e.status, 400);
    assert.equal(e.code, "VALIDATION_ERROR");
    assert.deepEqual(e.fields, { title: ["1~100자"] });
    assert.equal(e.requestId, "r-1");
    assert.deepEqual(fieldMessages(e), ["title: 1~100자"]);
    return true;
  });
});

test("401은 화면이 code로 분기할 수 있다", async () => {
  const f = fakeFetch(401, { error: { code: "UNAUTHORIZED", message: "토큰이 만료되었습니다" } });
  await assert.rejects(api("/api/auth/me", { fetchImpl: f }), (e) => e.status === 401 && e.code === "UNAUTHORIZED");
});

test("204는 본문 없이 성공", async () => {
  const f = fakeFetch(204, undefined);
  assert.deepEqual(await api("/api/posts/1", { method: "DELETE", fetchImpl: f }), { data: null });
});

test("네트워크 오류는 status 0 / NETWORK_ERROR", async () => {
  const f = async () => { throw new TypeError("Failed to fetch"); };
  await assert.rejects(api("/api/posts", { fetchImpl: f }), (e) => e.status === 0 && e.code === "NETWORK_ERROR");
});

test("JSON이 아닌 오류 응답(프록시 502 등)도 ApiError로", async () => {
  const f = async () => ({ status: 502, ok: false, text: async () => "<html>Bad Gateway</html>" });
  await assert.rejects(api("/api/posts", { fetchImpl: f }), (e) => e.status === 502 && e.code === "BAD_RESPONSE");
});

test("취소(AbortError)는 오류 화면을 띄우지 않도록 그대로 던진다", async () => {
  const f = async () => { const e = new Error("aborted"); e.name = "AbortError"; throw e; };
  await assert.rejects(api("/api/posts", { fetchImpl: f }), (e) => e.name === "AbortError");
});

test("쿠키 인증이면 credentials: include", async () => {
  const f = fakeFetch(200, { data: {} });
  await api("/api/auth/me", { credentials: true, fetchImpl: f });
  assert.equal(f.calls[0].init.credentials, "include");
});
