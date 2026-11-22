// study12 — 프론트의 API 클라이언트. (빌드 도구 없이 브라우저/Node에서 그대로 동작하는 ES 모듈)
//
// 공통 API 스펙 v1을 **한 곳에서** 해석한다.
//   성공: { data, meta? }                       → 호출한 쪽에는 { data, meta } 를 그대로 준다
//   실패: { error: { code, message, fields?, requestId? } } → ApiError 로 던진다
// 화면 코드는 `fetch`도 상태 코드도 JSON 모양도 모른 채 `api()`와 `ApiError`만 쓴다.

export class ApiError extends Error {
  constructor(status, error = {}) {
    super(error.message ?? `요청 실패 (${status})`);
    this.name = "ApiError";
    this.status = status;                       // HTTP 상태 (네트워크 오류면 0)
    this.code = error.code ?? "UNKNOWN";        // 기계가 읽는 코드 → 화면 분기는 이것으로
    this.fields = error.fields ?? null;         // { 필드명: [메시지, ...] } → 입력칸 아래에
    this.requestId = error.requestId ?? null;   // 문의/로그 추적용
  }
}

/**
 * @param {string} path           예: "/api/posts?page=1"
 * @param {object} [options]
 * @param {string} [options.method="GET"]
 * @param {any}    [options.body]            객체면 JSON으로 보낸다
 * @param {string} [options.token]           있으면 Authorization: Bearer
 * @param {string} [options.base=""]         API 서버 주소 (다른 출처일 때)
 * @param {boolean}[options.credentials]     쿠키 인증이면 true → credentials: "include"
 * @param {Function}[options.fetchImpl]      테스트에서 가짜 fetch를 넣는 자리
 */
export async function api(path, { method = "GET", body, token, base = "", credentials = false, signal, fetchImpl = fetch } = {}) {
  const headers = { Accept: "application/json" };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  if (token) headers.Authorization = `Bearer ${token}`;

  let res;
  try {
    res = await fetchImpl(base + path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: credentials ? "include" : "same-origin",
      signal,
    });
  } catch (cause) {
    // 서버에 닿지도 못한 경우 (오프라인, CORS 차단, 연결 거부). 상태 코드가 없다.
    if (cause?.name === "AbortError") throw cause;       // 사용자가 취소한 것은 오류로 취급하지 않는다
    throw new ApiError(0, { code: "NETWORK_ERROR", message: "서버에 연결할 수 없습니다." });
  }

  if (res.status === 204) return { data: null };         // 삭제 등 본문 없는 성공

  const text = await res.text();
  let json = null;
  try { json = text ? JSON.parse(text) : null; } catch { /* JSON이 아닌 응답 (프록시 오류 페이지 등) */ }

  if (!res.ok) {
    throw new ApiError(res.status, json?.error ?? { code: "BAD_RESPONSE", message: `요청 실패 (${res.status})` });
  }
  return json ?? { data: null };
}

/** 폼의 입력칸 아래에 오류를 보여 주기 위한 헬퍼. fields → "필드: 메시지" 한 줄들 */
export function fieldMessages(error) {
  return Object.entries(error.fields ?? {}).map(([name, msgs]) => `${name}: ${msgs.join(" ")}`);
}
