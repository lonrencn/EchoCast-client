/**
 * 把服务端下发文本里的链接找出来。
 *
 * 服务端没法保证下发的是纯 URL：经常是「已部署，访问 https://… 查看」
 * 这种夹在句子里的链接。这里只做识别、不做渲染 —— 组件拿分段结果自己画，
 * 避免 v-html 打开 XSS 口子。
 *
 * 认两种写法（与 `lib/open.ts` 的白名单一致，只有 http/https/mailto 能点开）：
 * 1. 裸链接：`https://…`（到空白或闭合括号为止）
 * 2. Markdown：`[说明文字](https://…)` —— 服务端想要可读文案时用这个
 */
export type TextSeg =
  | { kind: "text"; value: string }
  | { kind: "link"; value: string; label?: string };

const LINK_RE =
  /\[([^\]\n]{1,120})\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s<>"'）)\]]+)/g;

/**
 * 只判断「有没有」。必须用独立的非全局正则：
 * 带 /g 的正则调 .test() 会推进 lastIndex，matchAll 会从残留位置开始找，
 * 文本里只有一个链接时整段就被当成纯文本 —— 链接点不开的一半锅在这。
 */
const HAS_LINK_RE = new RegExp(LINK_RE.source);

export function linkify(text: string): TextSeg[] {
  if (!text) return [];
  const segs: TextSeg[] = [];
  let last = 0;
  for (const m of text.matchAll(LINK_RE)) {
    const at = m.index ?? 0;
    if (at > last) segs.push({ kind: "text", value: text.slice(last, at) });
    if (m[2]) {
      // Markdown 形式：[label](url)
      segs.push({ kind: "link", value: m[2], label: m[1] });
    } else {
      // 裸链接：把句尾的标点还给正文，「…访问 https://x.com。 」不该把句号算进链接
      let url = m[3];
      let trailing = "";
      while (/[.。,，;；!！?？]/.test(url.slice(-1))) {
        trailing = url.slice(-1) + trailing;
        url = url.slice(0, -1);
      }
      if (trailing) segs.push({ kind: "text", value: trailing });
      segs.push({ kind: "link", value: url });
    }
    last = at + m[0].length;
  }
  if (last < text.length) segs.push({ kind: "text", value: text.slice(last) });
  return segs;
}

/** 一段文本里有没有能点的链接。没有就用原来的纯文本渲染，省一层组件。 */
export function hasLink(text: string): boolean {
  return HAS_LINK_RE.test(text);
}
