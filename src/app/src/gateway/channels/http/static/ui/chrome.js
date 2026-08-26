/* Console chrome for pages that own their own markup.
 *
 * `mountShell` replaces `document.body`, which these pages cannot tolerate:
 * chat's classic script grabs #chat / #input at parse time and uses inline
 * handlers, and the trace viewer is a self-contained document. So the chrome is
 * wrapped around the existing children instead of replacing them, and the nav
 * comes from the same list as everywhere else.
 *
 * The page picks its layout with `<body data-chrome>`:
 *   "fill" — the shell hands over the remaining viewport and the page scrolls
 *            its own regions (chat).
 *   else   — the ordinary padded, page-scrolling shell (trace).
 */
import { brandHtml, markActiveNav, navHtml } from "./app.js";

/**
 * Move every current child of `body` into a shell `main`, with the topbar and
 * sidenav around it. Existing nodes are relocated, not recreated, so element
 * references and listeners taken before this runs stay valid.
 */
export function mountChrome({ fill } = {}) {
  if (fill) document.documentElement.classList.add("fill");

  const existing = Array.from(document.body.childNodes);
  // Relocating a node clears focus, and a page may focus a field before this
  // module runs, so the active element is restored afterwards. Otherwise the
  // caret is lost and the first keystroke goes nowhere.
  const focused = document.activeElement;

  const header = document.createElement("header");
  header.className = "topbar";
  header.innerHTML = brandHtml();

  const shell = document.createElement("div");
  shell.className = "shell" + (fill ? " fill" : "");
  const aside = document.createElement("aside");
  aside.className = "sidenav";
  aside.innerHTML = "<nav>" + navHtml() + "</nav>";
  const main = document.createElement("div");
  main.className = "main";

  existing.forEach((node) => main.appendChild(node));
  shell.appendChild(aside);
  shell.appendChild(main);
  document.body.appendChild(header);
  document.body.appendChild(shell);

  markActiveNav();

  if (focused && focused !== document.body && document.contains(focused)) {
    focused.focus();
  }
}

mountChrome({ fill: document.body.dataset.chrome === "fill" });
