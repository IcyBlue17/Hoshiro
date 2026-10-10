<script setup lang="ts">
import { onBeforeUnmount, type Directive } from "vue";
import { withBase } from "vitepress";
import HeroDemo from "./HeroDemo.vue";
import AppPreview from "./AppPreview.vue";
import CleanupDemo from "./CleanupDemo.vue";
import Pill from "./Pill.vue";
import Kbd from "./Kbd.vue";

const REPO = "https://github.com/IcyBlue17/hoshiro";
const DOWNLOAD = `${REPO}/releases/latest`;

// v-reveal: fade in once when scrolled into view
const observed = new Set<IntersectionObserver>();
const vReveal: Directive<HTMLElement, number | undefined> = {
  mounted(el, { value }) {
    el.classList.add("rv");
    if (value) el.style.transitionDelay = `${value}ms`;
    const io = new IntersectionObserver(([e]) => {
      if (!e.isIntersecting) return;
      el.classList.add("in");
      io.disconnect();
      observed.delete(io);
    }, { threshold: 0.15, rootMargin: "0px 0px -40px 0px" });
    io.observe(el);
    observed.add(io);
  },
};
onBeforeUnmount(() => observed.forEach((io) => io.disconnect()));

const MODES = [
  { name: "口述", keys: ["fn"], hint: "长按输入 / 双击免提", desc: "长按 Fn 说话，松开后自动将润色文本插入光标处。双击 Fn 锁定录音进入免提模式，适合长篇录入。" },
  { name: "翻译", keys: ["fn", "⇧"], hint: "即时外语翻译", desc: "口述源语言，系统直接转录并翻译为目标语言后插入。悬浮条支持快速切换目标语种。" },
  { name: "改写", keys: ["fn", "Space"], hint: "选中文本时触发", desc: "选中文本后口述指令（如“改礼貌点”、“精简表达”、“翻译成英文”），结果原地替换。" },
  { name: "问答", keys: ["fn", "Space"], hint: "未选中文本时触发", desc: "未选中文本时长按直接提问，回答以卡片形式呈现在屏幕底部，支持一键复制与 Markdown 渲染。" },
];

const LEARNED = ["Hoshiro", "Tauri", "TestFlight", "Q3 OKR", "张子涵", "Kubernetes", "飞书多维表格", "PR review"];
const APPS = [
  { name: "微信", pct: 36 },
  { name: "Slack", pct: 24 },
  { name: "VS Code", pct: 18 },
  { name: "邮件", pct: 13 },
  { name: "备忘录", pct: 9 },
];
const ASR = [
  { name: "小米 MiMo", note: "中文推荐，响应快自带标点" },
  { name: "Cloudflare Workers AI", note: "Whisper Large v3 Turbo" },
  { name: "Azure Speech", note: "流式传输，边说边转" },
  { name: "Google Cloud Speech", note: "流式传输" },
];
const LLM = [
  { name: "Google Gemini", note: "默认 Flash，兼顾速度与成本" },
  { name: "Anthropic Claude", note: "默认 Haiku，高准确率" },
  { name: "OpenAI 兼容接口", note: "DeepSeek、OpenRouter 及各类代理" },
];
</script>

<template>
  <div class="lp">
    <!-- Hero -->
    <section class="lp-hero">
      <div class="lp-wrap">
        <div class="lp-kicker">HOSHIRO · MACOS 语音输入法</div>
        <h1 class="lp-title">按住说话，<br />松开上屏。</h1>
        <p class="lp-sub">
          基于 Tauri 2 开发的 macOS 语音输入法。自动过滤口癖与停顿、修正改口并规范排版。<br />
          自带 API Key 直连服务商，无中间服务器，数据完全保存在本地。
        </p>
        <div class="lp-actions">
          <a class="btn btn-ink" :href="DOWNLOAD" target="_blank" rel="noreferrer">下载 macOS 版 (.dmg)</a>
          <a class="link-arrow" :href="withBase('/guide/getting-started')">查阅使用指南 →</a>
          <a class="link-arrow" :href="REPO" target="_blank" rel="noreferrer">GitHub 仓库 →</a>
        </div>

        <dl class="lp-spec">
          <div><dt>系统架构</dt><dd>macOS 13+（Apple Silicon 与 Intel 原生支持）</dd></div>
          <div><dt>技术栈</dt><dd>Tauri 2 (Rust) + React 19，本地直连 ASR 与 LLM</dd></div>
          <div><dt>隐私与存储</dt><dd>数据保存在本地 SQLite，API Key 存入系统钥匙串</dd></div>
          <div><dt>迁移兼容</dt><dd>完全兼容 Typeless 交互逻辑，支持一键导入配置与历史记录</dd></div>
        </dl>

        <div class="lp-block">
          <div class="lp-cap">
            <span class="lp-cap-label">界面预览</span>
            <span class="lp-cap-text">下面就是 Hoshiro 的主窗口，跑在演示数据上，可以直接点。</span>
          </div>
          <AppPreview />
        </div>

        <div class="lp-demo" style="margin-top: 48px;"><HeroDemo /></div>
      </div>
    </section>

    <!-- Cleanup -->
    <section class="lp-sec">
      <div class="lp-wrap lp-grid">
        <div v-reveal class="lp-side">
          <div class="lp-label">TEXT PROCESSING</div>
          <h2 class="lp-h2">口述成稿</h2>
          <p class="lp-p">无需刻意停顿组织语言。模型自动过滤口语助词与停顿，纠正改口重说，并将口语转为排版规范的书面语。</p>
        </div>
        <div v-reveal="80"><CleanupDemo /></div>
      </div>
    </section>

    <!-- Modes -->
    <section class="lp-sec">
      <div class="lp-wrap lp-grid">
        <div v-reveal class="lp-side">
          <div class="lp-label">INPUT MODES</div>
          <h2 class="lp-h2">四种输入模式</h2>
          <p class="lp-p">覆盖长按口述、长篇免提、实时翻译与就地改写。支持自定义独立修饰键（如 Fn、右 Option）、组合键及鼠标侧键。</p>
        </div>
        <div v-reveal="80" class="modes">
          <div v-for="m in MODES" :key="m.name" class="mode">
            <div class="mode-keys">
              <template v-for="(k, j) in m.keys" :key="k"><span v-if="j" class="plus">+</span><Kbd>{{ k }}</Kbd></template>
            </div>
            <div>
              <h3>{{ m.name }}</h3>
              <div class="mode-hint">{{ m.hint }}</div>
            </div>
            <p>{{ m.desc }}</p>
          </div>
        </div>
      </div>
    </section>

    <!-- Details -->
    <section class="lp-sec">
      <div class="lp-wrap lp-grid">
        <div v-reveal class="lp-side">
          <div class="lp-label">PRIVACY & STATS</div>
          <h2 class="lp-h2">本地优先与用量分析</h2>
          <p class="lp-p">无中心化账号，无云端服务器。所有数据与统计指标均在本地计算与存储。</p>
        </div>
        <div v-reveal="80" class="feats">
          <div class="feat">
            <div class="feat-art pill-art"><Pill cycle /></div>
            <h3>轻量悬浮胶囊</h3>
            <p>屏幕底部非模态悬浮条，录音时显示实时音量波形，处理阶段显示进度动效，不抢占窗口焦点。</p>
          </div>
          <div class="feat">
            <div class="feat-art learn-art">
              <span v-for="(w, n) in LEARNED" :key="w" class="chip" :style="{ animationDelay: 0.3 + n * 0.18 + 's' }">{{ w }}</span>
            </div>
            <h3>习惯自适应学习</h3>
            <p>自动学习常用专有名词、口癖与手动修正偏好。学习记录完全透明，支持逐条查阅与删除。</p>
          </div>
          <div class="feat">
            <div class="feat-art stats-art">
              <div v-for="(a, n) in APPS" :key="a.name" class="stat-row">
                <span class="stat-name">{{ a.name }}</span>
                <span class="stat-bar"><i :style="{ '--w': a.pct + '%', transitionDelay: 0.2 + n * 0.08 + 's' }" /></span>
                <span class="stat-pct">{{ a.pct }}%</span>
              </div>
            </div>
            <h3>按应用统计与延迟分析</h3>
            <p>统计各宿主应用输入字数占比；分段度量识别、改写与上屏耗时（均值 / 中位数 / P90），离线计算。</p>
          </div>
          <div class="feat">
            <div class="feat-art privacy-art">
              <span class="node">Mac 本地</span><span class="wire" /><span class="node">模型服务商</span>
            </div>
            <h3>纯本地存储与系统钥匙串</h3>
            <p>历史记录、音频切片与词典保存在本地 SQLite，API Key 存放于系统钥匙串，无外部中转。</p>
          </div>
        </div>
      </div>
    </section>

    <!-- Providers -->
    <section class="lp-sec">
      <div class="lp-wrap lp-grid">
        <div v-reveal class="lp-side">
          <div class="lp-label">PROVIDERS</div>
          <h2 class="lp-h2">直连各大模型服务</h2>
          <p class="lp-p">Bring Your Own Key。语音识别与文本润色模型解耦配置，支持自由组合。所有请求从本地直接发往对应服务商。</p>
        </div>
        <div v-reveal="80" class="prov">
          <div>
            <h3>语音识别 (ASR)</h3>
            <ul><li v-for="p in ASR" :key="p.name">{{ p.name }}<span>{{ p.note }}</span></li></ul>
          </div>
          <div>
            <h3>文本润色 (LLM)</h3>
            <ul><li v-for="p in LLM" :key="p.name">{{ p.name }}<span>{{ p.note }}</span></li></ul>
          </div>
        </div>
      </div>
      <div class="lp-wrap" style="margin-top: 48px;">
        <div v-reveal class="lp-shot">
          <img :src="withBase('/images/home.png')" alt="Hoshiro 主界面" loading="lazy" />
        </div>
      </div>
    </section>

    <!-- Migration -->
    <section class="lp-sec">
      <div class="lp-wrap lp-grid">
        <div v-reveal class="lp-side">
          <div class="lp-label">MIGRATION</div>
          <h2 class="lp-h2">平替 Typeless</h2>
          <p class="lp-p">快捷键逻辑与免提交互对标 Typeless，使用习惯无需改变。</p>
        </div>
        <div v-reveal="80">
          <p class="migrate" style="margin-top: 0; padding-top: 0; border-top: 0;">
            <b>从 Typeless 迁移？</b>可在应用内「设置 › 数据与迁移」一键导入快捷键绑定、语言偏好与历史记录；专有词典支持通过单列 CSV 导入。
            <a class="link-arrow" :href="withBase('/guide/migrate')">查看迁移说明 →</a>
          </p>
        </div>
      </div>
    </section>

    <!-- Final -->
    <section class="lp-final">
      <div class="lp-wrap">
        <h2 v-reveal class="lp-title">开始使用 Hoshiro</h2>
        <div class="lp-actions">
          <a class="btn btn-ink" :href="DOWNLOAD" target="_blank" rel="noreferrer">下载 macOS 版</a>
          <a class="link-arrow" :href="withBase('/guide/getting-started')">安装与配置指南 →</a>
          <a class="link-arrow" :href="REPO" target="_blank" rel="noreferrer">GitHub 源码 →</a>
        </div>
        <div class="final-pill"><Pill cycle /></div>
      </div>
    </section>
  </div>
</template>
