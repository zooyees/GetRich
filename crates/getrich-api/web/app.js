const BOARDS = [
  ["ALL", "全部"],
  ["CN", "沪深京"],
  ["CN.SH", "沪市"],
  ["CN.SZ", "深市"],
  ["CHINEXT", "创业板"],
  ["STAR", "科创板"],
  ["CN.BJ", "北证"],
  ["HK", "港股"],
  ["US", "美股"],
  ["WATCH", "自选"],
];

const READ_TIMEOUT_MS = 15000;
const READ_RETRIES = 3;

const state = {
  view: "home",
  board: "ALL",
  query: "",
  sort: "symbol",
  rows: [],
  cursor: 0,
  loading: false,
  stats: null,
  detail: null,
  bars: [],
  tf: "1d",
  empty_kind: null,
  empty_message: "",
  barEmpty: "",
  chartLoading: false,
  listError: null,
  detailError: null,
};

const $ = (id) => document.getElementById(id);

function fmt(n, d = 2) {
  if (n === null || n === undefined || Number.isNaN(n)) return "—";
  return Number(n).toLocaleString("zh-CN", { minimumFractionDigits: d, maximumFractionDigits: d });
}

function fmtVol(n) {
  if (n === null || n === undefined) return "—";
  const x = Number(n);
  if (Math.abs(x) >= 1e8) return (x / 1e8).toFixed(2) + "亿";
  if (Math.abs(x) >= 1e4) return (x / 1e4).toFixed(2) + "万";
  return x.toLocaleString("zh-CN");
}

function cls(dir) {
  if (dir === "up") return "chg-up";
  if (dir === "down") return "chg-down";
  return "chg-flat";
}

function signed(n, d = 2) {
  if (n === null || n === undefined) return "—";
  const v = Number(n);
  return (v > 0 ? "+" : "") + fmt(v, d);
}

async function fetchJson(url, options = {}, attempt = 1) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), READ_TIMEOUT_MS);
  try {
    const res = await fetch(url, { ...options, signal: ctrl.signal });
    const body = await res.json();
    if (!body.ok) {
      const err = new Error(body.error && body.error.message ? body.error.message : "请求失败");
      err.code = body.error && body.error.code;
      err.status = res.status;
      throw err;
    }
    return body.data;
  } catch (e) {
    const retryable =
      e.name === "AbortError" || e.message === "Failed to fetch" || (e.status && e.status >= 500);
    if (retryable && attempt < READ_RETRIES) {
      await new Promise((r) => setTimeout(r, 400 * attempt));
      return fetchJson(url, options, attempt + 1);
    }
    throw e;
  } finally {
    clearTimeout(timer);
  }
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

function setViews() {
  $("view-home").hidden = state.view !== "home";
  $("view-quotes").hidden = state.view !== "list";
  $("view-detail").hidden = state.view !== "detail";
  document.querySelectorAll(".nav a").forEach((a) => {
    const route = a.dataset.route;
    a.classList.toggle("active", (route === "home" && state.view === "home") || (route === "quotes" && state.view !== "home"));
  });
}

function setStatus(text) {
  $("status-pill").textContent = text;
}

function showFlash(id, html, isError) {
  const el = $(id);
  el.hidden = !html;
  el.classList.toggle("error", !!isError);
  el.innerHTML = html || "";
}

function renderBoards() {
  const nav = $("boards");
  nav.innerHTML = BOARDS.map(
    ([id, label]) =>
      `<button type="button" data-board="${id}" class="${state.board === id ? "on" : ""}">${label}</button>`
  ).join("");
  nav.querySelectorAll("button").forEach((b) => {
    b.onclick = () => {
      state.board = b.dataset.board;
      state.cursor = 0;
      renderBoards();
      loadList();
    };
  });
}

function renderHome() {
  setViews();
  const s = state.stats;
  if (!s) {
    $("home-listings").textContent = "—";
    $("home-quotes").textContent = "—";
    $("home-bars").textContent = "—";
    $("home-note").textContent = "";
    return;
  }
  $("home-listings").textContent = s.listings.toLocaleString("zh-CN");
  $("home-quotes").textContent = s.quotes.toLocaleString("zh-CN");
  $("home-bars").textContent = s.bars.toLocaleString("zh-CN");
  $("home-note").textContent = s.empty ? s.message || "空库可运行。导入后在此显示数量。" : "";
}

function renderList() {
  setViews();
  const wrap = $("quote-table-wrap");
  if (state.loading) {
    showFlash("quote-flash", "<h2>正在载入</h2><p>读取报价列表。</p>", false);
    wrap.hidden = true;
    return;
  }
  if (state.listError) {
    showFlash(
      "quote-flash",
      `<h2>无法载入行情</h2><p>${escapeHtml(state.listError)}</p><button type="button" class="btn btn-ghost retry" id="quote-retry">重试</button>`,
      true
    );
    $("quote-retry").onclick = () => loadList();
    wrap.hidden = true;
    return;
  }
  if (!state.rows.length) {
    const title = state.empty_kind === "no_match" ? "无匹配结果" : "暂无行情数据";
    showFlash(
      "quote-flash",
      `<h2>${title}</h2><p>${escapeHtml(state.empty_message || "导入 listings 与 quotes 后在此显示。")}</p>`,
      false
    );
    wrap.hidden = true;
    return;
  }
  showFlash("quote-flash", "", false);
  wrap.hidden = false;
  const body = $("quote-body");
  body.innerHTML = state.rows
    .map((r, i) => {
      const d = cls(r.direction);
      return `<tr data-i="${i}" class="${i === state.cursor ? "selected" : ""}">
        <td>${escapeHtml(r.symbol)}</td>
        <td>${escapeHtml(r.name || "—")}</td>
        <td class="${d}">${fmt(r.last)}</td>
        <td class="${d}">${r.change_pct == null ? "—" : signed(r.change_pct) + "%"}</td>
        <td class="${d}">${signed(r.change)}</td>
        <td>${fmt(r.open)}</td>
        <td class="${d}">${fmt(r.high)}</td>
        <td class="${d}">${fmt(r.low)}</td>
        <td>${fmt(r.prev_close)}</td>
        <td>${fmtVol(r.volume)}</td>
        <td>${fmtVol(r.turnover)}</td>
        <td>${r.amplitude == null ? "—" : fmt(r.amplitude) + "%"}</td>
        <td>${escapeHtml(r.market_label || r.market)}</td>
        <td>${escapeHtml(r.board_label || r.market_label)}</td>
      </tr>`;
    })
    .join("");
  body.querySelectorAll("tr").forEach((tr) => {
    tr.onclick = () => {
      state.cursor = Number(tr.dataset.i);
      openDetail();
    };
  });
}

function gridItem(label, value, dir) {
  return `<div><dt>${label}</dt><dd class="${dir || ""}">${value}</dd></div>`;
}

function renderDetail() {
  setViews();
  const d = state.detail;
  if (state.detailError) {
    showFlash(
      "detail-flash",
      `<h2>无法载入明细</h2><p>${escapeHtml(state.detailError)}</p><button type="button" class="btn btn-ghost retry" id="detail-retry">重试</button>`,
      true
    );
    const retry = $("detail-retry");
    if (retry) {
      retry.onclick = () => {
        const h = location.hash.match(/^#\/stock\/([^/]+)\/([^/]+)/);
        if (h) loadDetail(decodeURIComponent(h[1]), decodeURIComponent(h[2]));
      };
    }
    return;
  }
  if (!d) {
    showFlash("detail-flash", "<h2>正在载入</h2><p>读取标的明细。</p>", false);
    return;
  }
  showFlash("detail-flash", "", false);
  const q = d.quote;
  const dir = cls(q.direction);
  $("detail-name").textContent = q.name || q.symbol;
  $("detail-code").textContent = `${q.symbol}  ·  ${q.board_label || q.market_label}`;
  const last = d.empty && d.empty.no_quote ? "暂无报价" : fmt(q.last);
  $("quote-strip").innerHTML = [
    gridItem("最新", last, d.empty && d.empty.no_quote ? "chg-flat" : dir),
    gridItem("涨跌", signed(q.change), dir),
    gridItem("涨幅", q.change_pct == null ? "—" : signed(q.change_pct) + "%", dir),
    gridItem("今开", fmt(q.open), dir),
    gridItem("最高", fmt(q.high), dir),
    gridItem("最低", fmt(q.low), dir),
    gridItem("昨收", fmt(q.prev_close)),
    gridItem("成交量", fmtVol(q.volume)),
    gridItem("成交额", fmtVol(q.turnover)),
    gridItem("振幅", q.amplitude == null ? "—" : fmt(q.amplitude) + "%"),
  ].join("");
  drawChart();
}

function sma(values, n) {
  const out = new Array(values.length).fill(null);
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
    if (i >= n) sum -= values[i - n];
    if (i >= n - 1) out[i] = sum / n;
  }
  return out;
}

function drawChart() {
  const canvas = $("kline");
  const rows = state.bars || [];
  const ctx = canvas.getContext("2d");
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth || 800;
  const h = canvas.clientHeight || 420;
  canvas.width = Math.floor(w * dpr);
  canvas.height = Math.floor(h * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  if (state.chartLoading) {
    $("kline-meta").textContent = "正在载入 K 线";
    return;
  }
  if (!rows.length) {
    $("kline-meta").textContent = "";
    ctx.fillStyle = "#6e6e73";
    ctx.font = "15px -apple-system, BlinkMacSystemFont, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(state.barEmpty || "暂无 K 线。导入 bars 后在此绘制。", w / 2, h / 2);
    return;
  }

  const pad = { l: 56, r: 12, t: 16, b: 72 };
  const volH = 64;
  const plotH = h - pad.t - pad.b - volH;
  const plotW = w - pad.l - pad.r;
  const highs = rows.map((r) => r.high);
  const lows = rows.map((r) => r.low);
  const closes = rows.map((r) => r.close);
  let min = Math.min(...lows);
  let max = Math.max(...highs);
  const ma5 = sma(closes, 5);
  const ma10 = sma(closes, 10);
  const ma20 = sma(closes, 20);
  [ma5, ma10, ma20].forEach((arr) =>
    arr.forEach((v) => {
      if (v != null) {
        min = Math.min(min, v);
        max = Math.max(max, v);
      }
    })
  );
  if (min === max) {
    min -= 1;
    max += 1;
  }
  const vols = rows.map((r) => r.volume || 0);
  const vmax = Math.max(1, ...vols);
  const slot = plotW / rows.length;
  const y = (p) => pad.t + ((max - p) / (max - min)) * plotH;
  const x = (i) => pad.l + slot * i + slot / 2;

  ctx.strokeStyle = "rgba(0,0,0,0.08)";
  ctx.fillStyle = "#6e6e73";
  ctx.font = "11px ui-monospace, SFMono-Regular, monospace";
  ctx.textAlign = "right";
  for (let i = 0; i <= 4; i++) {
    const p = max - ((max - min) * i) / 4;
    const yy = y(p);
    ctx.fillText(p.toFixed(2), pad.l - 8, yy + 4);
    ctx.beginPath();
    ctx.moveTo(pad.l, yy);
    ctx.lineTo(w - pad.r, yy);
    ctx.stroke();
  }

  rows.forEach((r, i) => {
    const up = r.close >= r.open;
    ctx.strokeStyle = up ? "#e11d2f" : "#128a4b";
    ctx.fillStyle = up ? "#e11d2f" : "#128a4b";
    const cx = x(i);
    const bw = Math.max(1, slot * 0.7);
    ctx.beginPath();
    ctx.moveTo(cx, y(r.high));
    ctx.lineTo(cx, y(r.low));
    ctx.stroke();
    const top = y(Math.max(r.open, r.close));
    const bot = y(Math.min(r.open, r.close));
    ctx.fillRect(cx - bw / 2, top, bw, Math.max(1, bot - top));
    const vh = (vols[i] / vmax) * (volH - 8);
    const vy = h - 18 - vh;
    ctx.globalAlpha = 0.55;
    ctx.fillRect(cx - bw / 2, vy, bw, vh);
    ctx.globalAlpha = 1;
  });

  function strokeMa(arr, color) {
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.25;
    ctx.beginPath();
    let started = false;
    arr.forEach((v, i) => {
      if (v == null) return;
      if (!started) {
        ctx.moveTo(x(i), y(v));
        started = true;
      } else ctx.lineTo(x(i), y(v));
    });
    ctx.stroke();
    ctx.lineWidth = 1;
  }
  strokeMa(ma5, "#86868b");
  strokeMa(ma10, "#0071e3");
  strokeMa(ma20, "#bf4800");

  const last = rows[rows.length - 1];
  $("kline-meta").textContent = last
    ? `${last.ts}  开 ${fmt(last.open)}  高 ${fmt(last.high)}  低 ${fmt(last.low)}  收 ${fmt(last.close)}`
    : "";
}

async function loadHome() {
  state.view = "home";
  setViews();
  try {
    state.stats = await fetchJson("/v1/stats");
    setStatus(state.stats.empty ? "空库" : "已连接");
  } catch (e) {
    setStatus("未连接");
    state.stats = null;
    $("home-note").textContent = "无法读取库状态：" + e.message;
  }
  renderHome();
}

async function loadList() {
  state.view = "list";
  state.loading = true;
  state.listError = null;
  renderList();
  try {
    const q = new URLSearchParams({
      board: state.board,
      q: state.query,
      sort: state.sort,
      limit: "400",
    });
    const data = await fetchJson("/v1/quotes?" + q.toString());
    state.rows = data.rows || [];
    state.stats = data.stats;
    state.empty_kind = data.empty_kind;
    state.empty_message = data.empty_message;
    if (state.cursor >= state.rows.length) state.cursor = 0;
    setStatus(state.stats && state.stats.empty ? "空库" : "已连接");
  } catch (e) {
    state.listError = e.message;
    setStatus("未连接");
  } finally {
    state.loading = false;
    renderList();
  }
}

async function loadDetail(market, symbol) {
  state.view = "detail";
  state.chartLoading = true;
  state.detailError = null;
  state.detail = null;
  renderDetail();
  try {
    const detail = await fetchJson(`/v1/stocks/${encodeURIComponent(market)}/${encodeURIComponent(symbol)}`);
    state.detail = detail;
    renderDetail();
    const bars = await fetchJson(
      `/v1/stocks/${encodeURIComponent(market)}/${encodeURIComponent(symbol)}/bars?timeframe=${state.tf}&limit=500`
    );
    state.bars = bars.rows || [];
    state.barEmpty = bars.empty_message;
    setStatus("已连接");
  } catch (e) {
    if (e.code === "NOT_FOUND") {
      state.detailError = "未找到该股票。请确认已导入 listings。";
    } else {
      state.detailError = e.message;
    }
    setStatus("未连接");
  } finally {
    state.chartLoading = false;
    if (state.view === "detail") renderDetail();
  }
}

function openDetail() {
  const row = state.rows[state.cursor];
  if (!row) return;
  location.hash = `#/stock/${encodeURIComponent(row.market)}/${encodeURIComponent(row.symbol)}`;
}

function route() {
  const h = location.hash || "#/";
  const stock = h.match(/^#\/stock\/([^/]+)\/([^/]+)/);
  if (stock) {
    loadDetail(decodeURIComponent(stock[1]), decodeURIComponent(stock[2]));
    return;
  }
  if (h === "#/quotes" || h.startsWith("#/quotes?")) {
    loadList();
    return;
  }
  loadHome();
}

function bind() {
  renderBoards();
  $("quote-search").addEventListener("input", (e) => {
    state.query = e.target.value.trim();
    state.cursor = 0;
    loadList();
  });
  document.querySelectorAll(".quote-table th[data-sort]").forEach((th) => {
    th.style.cursor = "pointer";
    th.onclick = () => {
      state.sort = th.dataset.sort;
      loadList();
    };
  });
  $("tf-tabs").querySelectorAll("button").forEach((b) => {
    b.onclick = () => {
      state.tf = b.dataset.tf;
      $("tf-tabs").querySelectorAll("button").forEach((x) => x.classList.toggle("active", x === b));
      if (state.detail) loadDetail(state.detail.quote.market, state.detail.quote.symbol);
    };
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== $("quote-search")) {
      e.preventDefault();
      if (state.view !== "list") location.hash = "#/quotes";
      $("quote-search").focus();
      return;
    }
    if (state.view === "detail" && e.key === "Escape") {
      location.hash = "#/quotes";
      return;
    }
    if (state.view !== "list") return;
    if (e.key === "ArrowDown" || e.key === "j") {
      e.preventDefault();
      state.cursor = Math.min(state.rows.length - 1, state.cursor + 1);
      renderList();
    } else if (e.key === "ArrowUp" || e.key === "k") {
      e.preventDefault();
      state.cursor = Math.max(0, state.cursor - 1);
      renderList();
    } else if (e.key === "Enter") {
      openDetail();
    }
  });
  window.addEventListener("resize", () => {
    if (state.view === "detail") drawChart();
  });
}

bind();
window.addEventListener("hashchange", route);
route();
