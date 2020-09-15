import * as qs from "query-string";

const TagClass = "Tag";
const HiddenClass = "hidden";
const ArticleClass = "Article";

interface QueryStringRaw {
  [propName: string]: string | string[];
}

interface Tag {
  name: string;
}

function toggleSearchQueryProperty(
  queryString: QueryStringRaw,
  tag: Tag
): QueryStringRaw {
  Object.hasOwnProperty.call(queryString, tag.name)
    ? delete queryString[tag.name]
    : (queryString[tag.name] = null);
  return queryString;
}

function toggleTag(event: Event, target: HTMLElement) {
  event.preventDefault();
  let updatedQueryString = toggleSearchQueryProperty(
    qs.parse(location.search),
    { name: target.innerText }
  );
  let baseUrl = [
    location.protocol,
    "//",
    location.host,
    location.pathname,
    "?"
  ].join("");
  window.history.replaceState(
    {},
    "",
    baseUrl + qs.stringify(updatedQueryString)
  );
  updateView(updatedQueryString);
}

function highlightArticles(queryString: QueryStringRaw) {
  let selectedTags = Object.keys(queryString).concat(ArticleClass);
  let query: string = selectedTags
    .map(tag => `div:not(.${tag.trim()})`)
    .join(",");
  let articlesToHide: NodeListOf<Element> = query.length
    ? document.querySelectorAll(query)
    : document.querySelectorAll(null);
  let articlesToShow: HTMLCollectionOf<Element> = document.getElementsByClassName(
    selectedTags.join(" ")
  );
  forEach(articlesToHide, (element: HTMLElement) =>
    element.classList.add(HiddenClass)
  );
  forEach(articlesToShow, (element: HTMLElement) =>
    element.classList.remove(HiddenClass)
  );
}

function highlightTags(queryString: QueryStringRaw) {
  const tags: HTMLCollectionOf<Element> = document.getElementsByClassName(
    TagClass
  );
  forEach(tags, (element: HTMLElement) => highlightTag(queryString, element));
}

function highlightTag(queryString: QueryStringRaw, element: HTMLElement) {
  if (Object.hasOwnProperty.call(queryString, element.innerText))
    element.classList.add(`${TagClass}-selected`);
  else element.classList.remove(`${TagClass}-selected`);
}

function forEach(
  someCollection: HTMLCollectionOf<any> | NodeListOf<any>,
  f: Function
) {
  for (let index = 0; index < someCollection.length; index++) {
    f(someCollection[index], index);
  }
}

function init() {
  let queryString = qs.parse(location.search);
  forEach(document.getElementsByClassName(TagClass), (element: HTMLElement) => {
    element.addEventListener("click", ev => toggleTag(ev, element));
    highlightTag(queryString, element);
  });
  highlightArticles(queryString);
}

function updateView(queryString: QueryStringRaw) {
  highlightTags(queryString);
  highlightArticles(queryString);
}

document.addEventListener("DOMContentLoaded", init);
