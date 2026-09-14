// 프레임워크 없이 vanilla JS로 만든 아주 작은 SPA.
// - 빌드 도구가 필요 없어서 `cargo run` 하나로 전체 사이트가 뜬다.
// - 인증은 HttpOnly 쿠키라 JS가 토큰을 만질 필요가 없다 (credentials: same-origin).

const app = document.getElementById("app");
const nav = document.getElementById("nav");
const toastEl = document.getElementById("toast");

let me = null;

// ── 유틸 ────────────────────────────────────────────────────────────────
const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );

function toast(message, ms = 2600) {
  toastEl.textContent = message;
  toastEl.hidden = false;
  clearTimeout(toast._t);
  toast._t = setTimeout(() => (toastEl.hidden = true), ms);
}

function fmtDate(iso) {
  const d = new Date(iso);
  return isNaN(d) ? "" : d.toLocaleString("ko-KR", { dateStyle: "medium", timeStyle: "short" });
}

async function api(path, { method = "GET", body } = {}) {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
    credentials: "same-origin", // 세션 쿠키를 함께 보낸다
  });

  if (res.status === 204) return null;

  const data = await res.json().catch(() => null);
  if (!res.ok) {
    const err = new Error(data?.message ?? `요청 실패 (${res.status})`);
    err.status = res.status;
    err.code = data?.error;
    err.details = data?.details;
    throw err;
  }
  return data;
}

function renderFieldErrors(details) {
  if (!details) return "";
  const lines = Object.entries(details).flatMap(([field, msgs]) =>
    msgs.map((m) => `${field}: ${m}`),
  );
  return lines.length ? `<p class="field-errors">${lines.map(esc).join("<br>")}</p>` : "";
}

// ── 상단 내비게이션 ──────────────────────────────────────────────────────
function renderNav() {
  nav.innerHTML = me
    ? `<span class="who">${esc(me.username)} 님</span>
       <a class="btn" href="#/new">글쓰기</a>
       <button id="logout">로그아웃</button>`
    : `<a class="btn" href="#/login">로그인</a>
       <a class="btn" href="#/register">회원가입</a>`;

  const logout = document.getElementById("logout");
  if (logout) {
    logout.onclick = async () => {
      await api("/auth/logout", { method: "POST" });
      me = null;
      renderNav();
      toast("로그아웃했습니다.");
      location.hash = "#/";
      route();
    };
  }
}

// ── 화면들 ──────────────────────────────────────────────────────────────
async function viewList(params) {
  const page = Number(params.get("page") ?? 1);
  const q = params.get("q") ?? "";

  app.innerHTML = `<p class="muted">불러오는 중…</p>`;

  const search = new URLSearchParams({ page: String(page), per_page: "5" });
  if (q) search.set("q", q);

  let data;
  try {
    data = await api(`/posts?${search}`);
  } catch (e) {
    app.innerHTML = `<div class="empty">${esc(e.message)}</div>`;
    return;
  }

  const items = data.items
    .map(
      (p) => `
      <article class="card">
        <a href="#/posts/${p.id}">
          <h3>${esc(p.title)}</h3>
          <p class="meta">${esc(p.author_name)} · ${esc(fmtDate(p.created_at))}</p>
          <p class="excerpt">${esc(p.body.slice(0, 110))}${p.body.length > 110 ? "…" : ""}</p>
        </a>
      </article>`,
    )
    .join("");

  app.innerHTML = `
    <h1>게시판</h1>
    <p class="muted">전체 ${data.total}건 · ${data.page}/${Math.max(data.total_pages, 1)} 페이지</p>

    <form class="toolbar" id="search-form">
      <input type="search" name="q" value="${esc(q)}" placeholder="제목 또는 본문 검색" />
      <button class="primary" type="submit">검색</button>
      ${q ? `<a class="btn" href="#/">초기화</a>` : ""}
    </form>

    ${items || `<div class="empty">아직 글이 없습니다.</div>`}

    <div class="pager">
      <button ${page <= 1 ? "disabled" : ""} id="prev">이전</button>
      <span class="muted">${data.page} / ${Math.max(data.total_pages, 1)}</span>
      <button ${page >= data.total_pages ? "disabled" : ""} id="next">다음</button>
    </div>
  `;

  document.getElementById("search-form").onsubmit = (e) => {
    e.preventDefault();
    const value = new FormData(e.target).get("q").trim();
    location.hash = value ? `#/?q=${encodeURIComponent(value)}` : "#/";
  };

  const go = (p) => {
    const s = new URLSearchParams();
    if (q) s.set("q", q);
    if (p > 1) s.set("page", String(p));
    location.hash = s.toString() ? `#/?${s}` : "#/";
  };
  document.getElementById("prev").onclick = () => go(page - 1);
  document.getElementById("next").onclick = () => go(page + 1);
}

async function viewPost(id) {
  app.innerHTML = `<p class="muted">불러오는 중…</p>`;

  let post;
  try {
    post = await api(`/posts/${id}`);
  } catch (e) {
    app.innerHTML = `<div class="empty">${esc(e.message)}</div>`;
    return;
  }

  const mine = me && me.id === post.author_id;

  app.innerHTML = `
    <p class="muted"><a href="#/">← 목록</a></p>
    <h1>${esc(post.title)}</h1>
    <p class="muted">${esc(post.author_name)} · ${esc(fmtDate(post.created_at))}${
      post.updated_at !== post.created_at ? ` (수정됨 ${esc(fmtDate(post.updated_at))})` : ""
    }</p>
    <div class="post-body">${esc(post.body)}</div>
    ${
      mine
        ? `<div class="toolbar">
             <a class="btn" href="#/posts/${post.id}/edit">수정</a>
             <button class="danger" id="delete">삭제</button>
           </div>`
        : ""
    }
  `;

  const del = document.getElementById("delete");
  if (del) {
    del.onclick = async () => {
      if (!confirm("이 글을 삭제할까요?")) return;
      try {
        await api(`/posts/${post.id}`, { method: "DELETE" });
        toast("삭제했습니다.");
        location.hash = "#/";
      } catch (e) {
        toast(e.message);
      }
    };
  }
}

function viewEditor(post) {
  const editing = Boolean(post);

  app.innerHTML = `
    <h1>${editing ? "글 수정" : "새 글 쓰기"}</h1>
    <form id="post-form">
      <label><span>제목</span>
        <input name="title" maxlength="100" required value="${esc(post?.title ?? "")}" />
      </label>
      <label><span>본문</span>
        <textarea name="body" maxlength="5000" required>${esc(post?.body ?? "")}</textarea>
      </label>
      <div id="errors"></div>
      <div class="toolbar">
        <button class="primary" type="submit">${editing ? "저장" : "등록"}</button>
        <a class="btn" href="${editing ? `#/posts/${post.id}` : "#/"}">취소</a>
      </div>
    </form>
  `;

  document.getElementById("post-form").onsubmit = async (e) => {
    e.preventDefault();
    const form = new FormData(e.target);
    const payload = { title: form.get("title").trim(), body: form.get("body") };

    try {
      const saved = editing
        ? await api(`/posts/${post.id}`, { method: "PATCH", body: payload })
        : await api("/posts", { method: "POST", body: payload });

      toast(editing ? "수정했습니다." : "등록했습니다.");
      location.hash = `#/posts/${saved.id}`;
    } catch (err) {
      document.getElementById("errors").innerHTML =
        renderFieldErrors(err.details) || `<p class="field-errors">${esc(err.message)}</p>`;
    }
  };
}

function viewAuth(mode) {
  const isLogin = mode === "login";

  app.innerHTML = `
    <h1>${isLogin ? "로그인" : "회원가입"}</h1>
    <form class="auth" id="auth-form">
      <label><span>아이디</span><input name="username" required /></label>
      ${isLogin ? "" : `<label><span>이메일</span><input name="email" type="email" required /></label>`}
      <label><span>비밀번호</span><input name="password" type="password" required /></label>
      <div id="errors"></div>
      <div class="toolbar">
        <button class="primary" type="submit">${isLogin ? "로그인" : "가입하기"}</button>
        <a class="btn" href="${isLogin ? "#/register" : "#/login"}">${
          isLogin ? "회원가입" : "로그인"
        }</a>
      </div>
    </form>
  `;

  document.getElementById("auth-form").onsubmit = async (e) => {
    e.preventDefault();
    const form = new FormData(e.target);
    const payload = Object.fromEntries(form.entries());

    try {
      if (isLogin) {
        me = await api("/auth/login", { method: "POST", body: payload });
        renderNav();
        toast(`${me.username} 님, 환영합니다.`);
        location.hash = "#/";
      } else {
        await api("/auth/register", { method: "POST", body: payload });
        toast("가입했습니다. 로그인해 주세요.");
        location.hash = "#/login";
      }
    } catch (err) {
      document.getElementById("errors").innerHTML =
        renderFieldErrors(err.details) || `<p class="field-errors">${esc(err.message)}</p>`;
    }
  };
}

// ── 라우팅 (hash 기반) ───────────────────────────────────────────────────
async function route() {
  const raw = location.hash.slice(1) || "/";
  const [path, queryString] = raw.split("?");
  const params = new URLSearchParams(queryString);

  const editMatch = path.match(/^\/posts\/(\d+)\/edit$/);
  const postMatch = path.match(/^\/posts\/(\d+)$/);

  if (path === "/") return viewList(params);
  if (path === "/login") return viewAuth("login");
  if (path === "/register") return viewAuth("register");

  if (path === "/new") {
    if (!me) {
      toast("로그인이 필요합니다.");
      location.hash = "#/login";
      return;
    }
    return viewEditor(null);
  }

  if (editMatch) {
    try {
      const post = await api(`/posts/${editMatch[1]}`);
      if (!me || me.id !== post.author_id) {
        toast("본인이 작성한 글만 수정할 수 있습니다.");
        location.hash = `#/posts/${post.id}`;
        return;
      }
      return viewEditor(post);
    } catch (e) {
      app.innerHTML = `<div class="empty">${esc(e.message)}</div>`;
      return;
    }
  }

  if (postMatch) return viewPost(postMatch[1]);

  app.innerHTML = `<div class="empty">없는 페이지입니다. <a href="#/">목록으로</a></div>`;
}

window.addEventListener("hashchange", route);

(async function start() {
  try {
    me = await api("/auth/me"); // 쿠키가 있으면 로그인 상태 복원
  } catch {
    me = null;
  }
  renderNav();
  route();
})();
