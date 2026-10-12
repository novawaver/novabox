import DOMPurify from "dompurify";

export type CleanMail = {
  html: string;
  warnings: string[];
  blockedImages: number;
};

export function cleanFragment(input: string, allowRemote: boolean): string {
  const cleaned = cleanEmail(input, allowRemote);
  const doc = new DOMParser().parseFromString(cleaned.html, "text/html");
  return doc.body.innerHTML;
}

export function cleanEmail(input: string, allowRemote: boolean): CleanMail {
  const sanitized = DOMPurify.sanitize(input || "", {
    USE_PROFILES: { html: true },
    ADD_TAGS: ["style"],
    ADD_ATTR: ["target", "align", "bgcolor", "valign", "cellpadding", "cellspacing", "border"],
    FORBID_TAGS: [
      "script",
      "iframe",
      "object",
      "embed",
      "form",
      "input",
      "button",
      "textarea",
      "select",
      "option",
      "link",
      "meta",
      "base",
      "svg",
      "math",
    ],
    FORBID_ATTR: ["srcset", "action", "formaction"],
  });
  const doc = new DOMParser().parseFromString(`<div id="novabox-root">${sanitized}</div>`, "text/html");
  const root = doc.getElementById("novabox-root");
  if (!root) return { html: "", warnings: [], blockedImages: 0 };

  let blockedImages = 0;
  const warnings: string[] = [];
  root.querySelectorAll("img").forEach((img) => {
    const src = img.getAttribute("src") || "";
    if (/^https?:/i.test(src) || src.startsWith("//")) {
      if (!allowRemote) {
        img.removeAttribute("src");
        img.setAttribute("alt", img.getAttribute("alt") || "Remote image blocked");
        blockedImages += 1;
      }
    } else if (src && !src.startsWith("data:")) {
      img.removeAttribute("src");
    }
  });

  root.querySelectorAll<HTMLElement>("*").forEach((el) => {
    const style = el.getAttribute("style");
    if (style) el.setAttribute("style", scrubCss(style, allowRemote));
    const background = el.getAttribute("background");
    if (background && /^https?:/i.test(background) && !allowRemote) el.removeAttribute("background");
  });
  root.querySelectorAll("style").forEach((style) => {
    style.textContent = scrubCss(style.textContent || "", allowRemote).replace(/@import/gi, "");
  });

  root.querySelectorAll("a").forEach((link) => {
    const href = link.getAttribute("href") || "";
    if (!/^https?:/i.test(href) && !/^mailto:/i.test(href)) {
      link.removeAttribute("href");
      return;
    }
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noreferrer noopener");
    const text = (link.textContent || "").trim();
    if (/^(https?:\/\/|www\.)/i.test(text)) {
      try {
        const shown = new URL(text.startsWith("www.") ? `https://${text}` : text);
        const real = new URL(href);
        const host = (value: string) => value.replace(/^www\./, "");
        if (host(shown.hostname) !== host(real.hostname)) {
          warnings.push(`“${trimText(text)}” goes to ${real.hostname}`);
        }
      } catch {
        warnings.push(`“${trimText(text)}” does not match where the link goes`);
      }
    }
  });

  const imgSrc = allowRemote ? "data: https: http:" : "data:";
  const html = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src ${imgSrc}; form-action 'none';"><style>html,body{margin:0;padding:0}body{font:15px/1.55 "Segoe UI",sans-serif;color:#1e1a15;overflow-wrap:anywhere}img,video{max-width:100%;height:auto}a{color:#8d4e09}blockquote{margin:8px 0 8px .4rem;padding-left:.7rem;border-left:2px solid #e0d8cb;color:#444}pre{white-space:pre-wrap;font:inherit}</style></head><body>${root.innerHTML}</body></html>`;
  return { html, warnings, blockedImages };
}

function scrubCss(css: string, allowRemote: boolean): string {
  const withoutExpression = css.replace(/expression\s*\(/gi, "");
  if (allowRemote) return withoutExpression;
  return withoutExpression.replace(/url\s*\(\s*['"]?\s*(?:https?:|\/\/)[^)]+\)/gi, "none");
}

function trimText(value: string): string {
  return value.length > 80 ? `${value.slice(0, 77)}…` : value;
}

export function looksLikeHtml(value: string): boolean {
  return /<\/?[a-z][\s\S]*>/i.test(value);
}
