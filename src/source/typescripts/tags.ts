const TagClass = "Tag";
const HiddenClass = "hidden";
const ArticleClass = "Article";

function tagName(element: HTMLElement): string {
  return (element.textContent || "").trim();
}

function toggleTagInQuery(
  query: URLSearchParams,
  name: string,
): URLSearchParams {
  if (query.has(name)) query.delete(name);
  else query.set(name, "");
  return query;
}

function toggleTag(event: Event, target: HTMLElement): void {
  event.preventDefault();
  const query = toggleTagInQuery(
    new URLSearchParams(location.search),
    tagName(target),
  );
  const baseUrl = `${location.protocol}//${location.host}${location.pathname}?`;
  window.history.replaceState({}, "", baseUrl + query.toString());
  updateView(query);
}

function highlightArticles(query: URLSearchParams): void {
  const tags = Array.from(query.keys()).map((tag) => tag.trim());

  if (tags.length) {
    const stale = tags
      .map((tag) => `.${ArticleClass}:not(.${CSS.escape(tag)})`)
      .join(",");
    Array.from(document.querySelectorAll<HTMLElement>(stale)).forEach((el) =>
      el.classList.add(HiddenClass),
    );
  }

  const matching = document.getElementsByClassName(
    [ArticleClass, ...tags].join(" "),
  );
  Array.from(matching).forEach((el) => el.classList.remove(HiddenClass));
}

function highlightTags(query: URLSearchParams): void {
  Array.from(document.getElementsByClassName(TagClass)).forEach((el) =>
    highlightTag(query, el as HTMLElement),
  );
}

function highlightTag(query: URLSearchParams, element: HTMLElement): void {
  element.classList.toggle(`${TagClass}-selected`, query.has(tagName(element)));
}

function updateView(query: URLSearchParams): void {
  highlightTags(query);
  highlightArticles(query);
}

function init(): void {
  const query = new URLSearchParams(location.search);
  Array.from(document.getElementsByClassName(TagClass)).forEach((el) => {
    const tag = el as HTMLElement;
    tag.addEventListener("click", (event) => toggleTag(event, tag));
    highlightTag(query, tag);
  });
  highlightArticles(query);
  // enable transitions only after the initial filter, so the on-load state doesn't visibly fade
  document.documentElement.classList.add("ready");
}

document.addEventListener("DOMContentLoaded", init);
