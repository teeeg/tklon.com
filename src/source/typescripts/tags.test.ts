import { beforeEach, describe, expect, it } from "vitest";

// Import once so the module's DOMContentLoaded -> init listener is registered a
// single time. Each test re-renders the DOM and re-dispatches DOMContentLoaded,
// so init runs once per test against fresh elements.
import "./tags";

function makeTag(name: string): HTMLAnchorElement {
  const anchor = document.createElement("a");
  anchor.className = "Tag";
  anchor.href = `/tags/?${name}`;
  anchor.textContent = name;
  return anchor;
}

function makeArticle(text: string, tags: string[]): HTMLDivElement {
  const div = document.createElement("div");
  div.className = ["Article", ...tags].join(" ");
  div.textContent = text;
  return div;
}

function render(): void {
  document.body.replaceChildren(
    makeTag("books"),
    makeTag("film"),
    makeArticle("Book post", ["books"]),
    makeArticle("Film post", ["film"]),
    makeArticle("Both post", ["books", "film"])
  );
}

const tag = (name: string) =>
  Array.from(document.getElementsByClassName("Tag")).find(
    el => el.textContent?.trim() === name
  ) as HTMLElement;

const article = (text: string) =>
  Array.from(document.getElementsByClassName("Article")).find(
    el => el.textContent?.includes(text)
  ) as HTMLElement;

const isFaded = (text: string) => article(text).classList.contains("hidden");

beforeEach(() => {
  window.history.replaceState({}, "", "/tags/");
  render();
  document.dispatchEvent(new Event("DOMContentLoaded"));
});

describe("tag filtering", () => {
  it("shows every article and selects no tag initially", () => {
    expect(isFaded("Book post")).toBe(false);
    expect(isFaded("Film post")).toBe(false);
    expect(isFaded("Both post")).toBe(false);
    expect(document.querySelector(".Tag-selected")).toBeNull();
  });

  it("fades non-matching articles and reflects the tag in the URL on click", () => {
    tag("books").click();

    expect(tag("books").classList.contains("Tag-selected")).toBe(true);
    expect(window.location.search).toContain("books");

    expect(isFaded("Book post")).toBe(false); // has books
    expect(isFaded("Both post")).toBe(false); // has books + film
    expect(isFaded("Film post")).toBe(true); // missing books
  });

  it("narrows to the intersection when two tags are selected", () => {
    tag("books").click();
    tag("film").click();

    expect(isFaded("Both post")).toBe(false); // has both tags
    expect(isFaded("Book post")).toBe(true); // missing film
    expect(isFaded("Film post")).toBe(true); // missing books
  });

  it("restores everything when a tag is toggled back off", () => {
    tag("books").click(); // on
    tag("books").click(); // off

    expect(tag("books").classList.contains("Tag-selected")).toBe(false);
    expect(window.location.search).not.toContain("books");
    expect(isFaded("Book post")).toBe(false);
    expect(isFaded("Film post")).toBe(false);
    expect(isFaded("Both post")).toBe(false);
  });

  it("handles tag names with CSS-special characters without throwing", () => {
    document.body.replaceChildren(
      makeTag("c++"),
      makeArticle("Cpp post", ["c++"]),
      makeArticle("Other post", ["books"])
    );
    document.dispatchEvent(new Event("DOMContentLoaded"));

    expect(() => tag("c++").click()).not.toThrow();
    expect(isFaded("Cpp post")).toBe(false); // matches the selected tag
    expect(isFaded("Other post")).toBe(true); // missing it
  });
});
