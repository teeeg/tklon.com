// jsdom does not implement the CSS interface, so `CSS.escape` (used by
// tags.ts and available natively in every targeted browser) is missing in the
// test environment. Polyfill it with the spec algorithm so tests exercise the
// same escaping as production.
// https://drafts.csswg.org/cssom/#serialize-an-identifier
if (
  typeof globalThis.CSS === "undefined" ||
  typeof globalThis.CSS.escape !== "function"
) {
  const cssEscape = (value: string): string => {
    const string = String(value);
    const length = string.length;
    const firstCodeUnit = string.charCodeAt(0);
    let result = "";
    let index = -1;

    while (++index < length) {
      const codeUnit = string.charCodeAt(index);

      if (codeUnit === 0x0000) {
        result += "�";
      } else if (
        (codeUnit >= 0x0001 && codeUnit <= 0x001f) ||
        codeUnit === 0x007f ||
        (index === 0 && codeUnit >= 0x0030 && codeUnit <= 0x0039) ||
        (index === 1 &&
          codeUnit >= 0x0030 &&
          codeUnit <= 0x0039 &&
          firstCodeUnit === 0x002d)
      ) {
        result += "\\" + codeUnit.toString(16) + " ";
      } else if (index === 0 && length === 1 && codeUnit === 0x002d) {
        result += "\\" + string.charAt(index);
      } else if (
        codeUnit >= 0x0080 ||
        codeUnit === 0x002d ||
        codeUnit === 0x005f ||
        (codeUnit >= 0x0030 && codeUnit <= 0x0039) ||
        (codeUnit >= 0x0041 && codeUnit <= 0x005a) ||
        (codeUnit >= 0x0061 && codeUnit <= 0x007a)
      ) {
        result += string.charAt(index);
      } else {
        result += "\\" + string.charAt(index);
      }
    }
    return result;
  };

  globalThis.CSS = {
    ...(globalThis.CSS || {}),
    escape: cssEscape,
  } as typeof globalThis.CSS;
}
