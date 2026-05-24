import * as qs from "query-string";

const TagClass = "Tag";
const HiddenClass = "hidden";
const ArticleClass = "Article";

type QueryString = qs.ParsedQuery;

function tagName(element: HTMLElement): string {
  return (element.textContent || "").trim();
}

function toggleTagInQuery(query: QueryString, name: string): QueryString {
  if (Object.prototype.hasOwnProperty.call(query, name)) delete query[name];
  else query[name] = null;
  return query;
}

function toggleTag(event: Event, target: HTMLElement): void {
  event.preventDefault();
  const query = toggleTagInQuery(qs.parse(location.search), tagName(target));
  const baseUrl = `${location.protocol}//${location.host}${location.pathname}?`;
  window.history.replaceState({}, "", baseUrl + qs.stringify(query));
  updateView(query);
}

function highlightArticles(query: QueryString): void {
  const tags = Object.keys(query).map(tag => tag.trim());

  // Fade any article that is missing at least one selected tag.
  if (tags.length) {
    const stale = tags.map(tag => `.${ArticleClass}:not(.${tag})`).join(",");
    Array.from(document.querySelectorAll<HTMLElement>(stale)).forEach(el =>
      el.classList.add(HiddenClass)
    );
  }

  // Un-fade the articles that match every selected tag.
  const matching = document.getElementsByClassName([ArticleClass, ...tags].join(" "));
  Array.from(matching).forEach(el => el.classList.remove(HiddenClass));
}

function highlightTags(query: QueryString): void {
  Array.from(document.getElementsByClassName(TagClass)).forEach(el =>
    highlightTag(query, el as HTMLElement)
  );
}

function highlightTag(query: QueryString, element: HTMLElement): void {
  const selected = Object.prototype.hasOwnProperty.call(query, tagName(element));
  element.classList.toggle(`${TagClass}-selected`, selected);
}

function updateView(query: QueryString): void {
  highlightTags(query);
  highlightArticles(query);
}

function init(): void {
  const query = qs.parse(location.search);
  Array.from(document.getElementsByClassName(TagClass)).forEach(el => {
    const tag = el as HTMLElement;
    tag.addEventListener("click", event => toggleTag(event, tag));
    highlightTag(query, tag);
  });
  highlightArticles(query);
}

document.addEventListener("DOMContentLoaded", init);
