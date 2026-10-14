/*
 * quarto-search.js
 * Copyright (c) 2026 Posit, PBC
 *
 * Website search UI for Quarto 2 (bd-eip0naf4).
 *
 * A port of Quarto 1's quarto-search.js (quarto-cli
 * src/resources/projects/website/search/quarto-search.js) with the
 * engine swapped: Quarto 1 fetched the whole search.json and scanned it
 * with Fuse.js; this file lazily imports the Pagefind runtime from the
 * bundle `q2 render` wrote next to the index and lets it fetch only the
 * chunks a query touches. Everything the reader sees — the Algolia
 * Autocomplete widget, grouping by document with `collapse-after`,
 * crumbs, the keyboard shortcut, the copy button, `?q=` highlighting on
 * the target page and the tabset handling that goes with it — is kept
 * from Quarto 1.
 *
 * Deviations from Quarto 1, each deliberate:
 * - `readSearchData` / `fuseSearch` / the Fuse sub-index workaround are
 *   gone; `pagefindSearch` replaces them. Pagefind's excerpts (already
 *   highlighted with <mark>) replace `highlightMatch` for the Pagefind
 *   path; `highlightMatch` is kept for the Algolia path.
 * - `offsetURL` passes absolute and root-relative URLs through: Pagefind
 *   result URLs are absolute (built from the site root), and federated
 *   results point at other sites.
 * - Federated search (`federate` in the options): other sites' indexes
 *   are merged with Pagefind's `mergeIndex` after a manifest check, and
 *   each result card can show which site it came from.
 * - The `file://` alert became an error card in the results panel.
 * - The copy button falls back to `navigator.clipboard` when
 *   ClipboardJS is not on the page.
 * - `positionPanel` only sets `top`; Quarto 1 also assigned `left` /
 *   `right` from properties that do not exist on DOM elements (a no-op).
 */

const kQueryArg = "q";
const kResultsArg = "show-results";

// If items don't provide a URL, then both the navigator and the onSelect
// function aren't called (and therefore, the default implementation is used)
//
// We're using this sentinel URL to signal to those handlers that this
// item is a more item (along with the type) and can be handled appropriately
const kItemTypeMoreHref = "0767FDFD-0422-4E5A-BC8A-3BE11E5BBA05";

// Capture search params and clean ?q= from URL at module load time, before
// any DOMContentLoaded handlers run. quarto-nav.js resolves all <a> hrefs
// against window.location during DOMContentLoaded — if ?q= is still present,
// every link on the page gets the query param baked into its href.
const currentUrl = new URL(window.location);
const kQuery = currentUrl.searchParams.get(kQueryArg);
if (kQuery) {
  const replacementUrl = new URL(window.location);
  replacementUrl.searchParams.delete(kQueryArg);
  window.history.replaceState({}, "", replacementUrl);
}

window.document.addEventListener("DOMContentLoaded", function (_event) {
  // Ensure that search is available on this page. If it isn't,
  // should return early and not do anything
  var searchEl = window.document.getElementById("quarto-search");
  if (!searchEl) return;

  const autocompleteLib = window["@algolia/autocomplete-js"];
  if (!autocompleteLib) {
    console.error("quarto-search: the autocomplete widget did not load");
    return;
  }
  const { autocomplete } = autocompleteLib;

  let quartoSearchOptions = {};
  let language = {};
  const searchOptionEl = window.document.getElementById(
    "quarto-search-options"
  );
  if (searchOptionEl) {
    const jsonStr = searchOptionEl.textContent;
    quartoSearchOptions = JSON.parse(jsonStr);
    language = quartoSearchOptions.language || {};
  }

  // note the search mode
  if (quartoSearchOptions.type === "overlay") {
    searchEl.classList.add("type-overlay");
  } else {
    searchEl.classList.add("type-textbox");
  }

  // Used to determine highlighting behavior for this page
  // A `q` query param is expected when the user follows a search
  // to this page
  const query = kQuery;
  const showSearchResults = currentUrl.searchParams.get(kResultsArg);
  const mainEl = window.document.querySelector("main");

  // highlight matches on the page
  if (query && mainEl) {
    highlight(query, mainEl);

    // Activate tabs on pageshow — after tabsets.js restores localStorage state.
    // tabsets.js registers its pageshow handler during module execution (before
    // DOMContentLoaded). By registering ours during DOMContentLoaded, listener
    // ordering guarantees we run after tabsets.js — so search activation wins.
    window.addEventListener(
      "pageshow",
      function (event) {
        if (!event.persisted) {
          for (const mark of mainEl.querySelectorAll("mark")) {
            openAllTabsetsContainingEl(mark);
          }
          // Only scroll to first match when there's no hash fragment.
          // With a hash, the browser already scrolled to the target section.
          if (!currentUrl.hash) {
            requestAnimationFrame(() => scrollToFirstVisibleMatch(mainEl));
          }
        }
      },
      { once: true }
    );
  }

  // function to clear highlighting on the page when the search query changes
  // (e.g. if the user edits the query or clears it)
  let highlighting = true;
  const resetHighlighting = (searchTerm) => {
    if (mainEl && highlighting && query && searchTerm !== query) {
      clearHighlight(query, mainEl);
      highlighting = false;
    }
  };

  // Responsively switch to overlay mode if the search is present on the navbar
  // Note that switching the sidebar to overlay mode requires more coordinate (not just
  // the media query since we generate different HTML for sidebar overlays than we do
  // for sidebar input UI)
  const detachedMediaQuery =
    quartoSearchOptions.type === "overlay" ? "all" : "(max-width: 991px)";

  // If configured, include the analytics client to send insights
  const plugins = configurePlugins(quartoSearchOptions);

  let lastState = null;
  const { setIsOpen, setQuery, setCollections } = autocomplete({
    container: searchEl,
    detachedMediaQuery: detachedMediaQuery,
    defaultActiveItemId: 0,
    panelContainer: "#quarto-search-results",
    panelPlacement: quartoSearchOptions["panel-placement"],
    debug: false,
    openOnFocus: true,
    plugins,
    classNames: {
      form: "d-flex",
    },
    placeholder: language["search-text-placeholder"],
    translations: {
      clearButtonTitle: language["search-clear-button-title"],
      detachedCancelButtonText: language["search-detached-cancel-button-title"],
      submitButtonTitle: language["search-submit-button-title"],
    },
    initialState: {
      query,
    },
    getItemUrl({ item }) {
      return item.href;
    },
    onStateChange({ state }) {
      // Perhaps reset highlighting
      resetHighlighting(state.query);

      // If the panel just opened, ensure the panel is positioned properly
      if (state.isOpen) {
        if (lastState && !lastState.isOpen) {
          setTimeout(() => {
            positionPanel(quartoSearchOptions["panel-placement"]);
          }, 150);
        }
      }

      // Perhaps show the copy link
      showCopyLink(state.query, quartoSearchOptions);

      lastState = state;
    },
    reshape({ sources, state }) {
      return sources.map((source) => {
        try {
          const items = source.getItems();

          // Validate the items
          validateItems(items);

          // group the items by document
          const groupedItems = new Map();
          items.forEach((item) => {
            const hrefParts = item.href.split("#");
            const baseHref = hrefParts[0];
            const isDocumentItem = hrefParts.length === 1;

            const items = groupedItems.get(baseHref);
            if (!items) {
              groupedItems.set(baseHref, [item]);
            } else {
              // If the href for this item matches the document
              // exactly, place this item first as it is the item that represents
              // the document itself
              if (isDocumentItem) {
                items.unshift(item);
              } else {
                items.push(item);
              }
              groupedItems.set(baseHref, items);
            }
          });

          const reshapedItems = [];
          let count = 1;
          for (const [_key, value] of groupedItems) {
            const firstItem = value[0];
            reshapedItems.push({
              ...firstItem,
              type: kItemTypeDoc,
            });

            const collapseMatches = quartoSearchOptions["collapse-after"];
            const collapseCount =
              typeof collapseMatches === "number" ? collapseMatches : 1;

            if (value.length > 1) {
              const target = `search-more-${count}`;
              const isExpanded =
                state.context.expanded &&
                state.context.expanded.includes(target);

              const remainingCount = value.length - collapseCount;

              for (let i = 1; i < value.length; i++) {
                if (collapseMatches && i === collapseCount) {
                  reshapedItems.push({
                    target,
                    title: isExpanded
                      ? language["search-hide-matches-text"]
                      : remainingCount === 1
                        ? `${remainingCount} ${language["search-more-match-text"]}`
                        : `${remainingCount} ${language["search-more-matches-text"]}`,
                    type: kItemTypeMore,
                    href: kItemTypeMoreHref,
                  });
                }

                if (isExpanded || !collapseMatches || i < collapseCount) {
                  reshapedItems.push({
                    ...value[i],
                    type: kItemTypeItem,
                    target,
                  });
                }
              }
            }
            count += 1;
          }

          return {
            ...source,
            getItems() {
              return reshapedItems;
            },
          };
        } catch (error) {
          // Some form of error occurred
          return {
            ...source,
            getItems() {
              return [
                {
                  title: error.name || "An Error Occurred While Searching",
                  text:
                    error.message ||
                    "An unknown error occurred while attempting to perform the requested search.",
                  type: kItemTypeError,
                },
              ];
            },
          };
        }
      });
    },
    navigator: {
      navigate({ itemUrl }) {
        if (itemUrl !== offsetURL(kItemTypeMoreHref)) {
          window.location.assign(itemUrl);
        }
      },
      navigateNewTab({ itemUrl }) {
        if (itemUrl !== offsetURL(kItemTypeMoreHref)) {
          const windowReference = window.open(itemUrl, "_blank", "noopener");
          if (windowReference) {
            windowReference.focus();
          }
        }
      },
      navigateNewWindow({ itemUrl }) {
        if (itemUrl !== offsetURL(kItemTypeMoreHref)) {
          window.open(itemUrl, "_blank", "noopener");
        }
      },
    },
    getSources({ state, setContext, setActiveItemId, refresh }) {
      return [
        {
          sourceId: "documents",
          getItemUrl({ item }) {
            if (item.href) {
              return offsetURL(item.href);
            } else {
              return undefined;
            }
          },
          onSelect({
            item,
            state,
            setContext,
            setIsOpen,
            setActiveItemId,
            refresh,
          }) {
            if (item.type === kItemTypeMore) {
              toggleExpanded(item, state, setContext, setActiveItemId, refresh);

              // Toggle more
              setIsOpen(true);
            }
          },
          getItems({ query }) {
            if (query === null || query === "") {
              return [];
            }

            const limit = quartoSearchOptions.limit;
            if (quartoSearchOptions.algolia) {
              return algoliaSearch(query, limit, quartoSearchOptions.algolia);
            } else {
              return pagefindSearch(query, limit, quartoSearchOptions);
            }
          },
          templates: {
            noResults({ createElement }) {
              const hasQuery = lastState.query;

              return createElement(
                "div",
                {
                  class: `quarto-search-no-results${hasQuery ? "" : " no-query"}`,
                },
                language["search-no-results-text"]
              );
            },
            header({ items, createElement }) {
              // count the documents
              const count = items.filter((item) => {
                return item.type === kItemTypeDoc;
              }).length;

              if (count > 0) {
                return createElement(
                  "div",
                  { class: "search-result-header" },
                  `${count} ${language["search-matching-documents-text"]}`
                );
              } else {
                return createElement(
                  "div",
                  { class: "search-result-header-no-results" },
                  ``
                );
              }
            },
            footer({ _items, createElement }) {
              if (
                quartoSearchOptions.algolia &&
                quartoSearchOptions.algolia["show-logo"]
              ) {
                const libDir = quartoSearchOptions.algolia["libDir"];
                const logo = createElement("img", {
                  src: offsetURL(
                    `${libDir}/quarto-search/search-by-algolia.svg`
                  ),
                  class: "algolia-search-logo",
                });
                return createElement(
                  "a",
                  { href: "http://www.algolia.com/" },
                  logo
                );
              }
            },

            item({ item, createElement }) {
              if (item.text && item.href && !item.href.includes("?q=")) {
                const [main, hash] = item.href.split("#");
                const hashAppend = hash ? "#" + hash : "";
                item.href =
                  main + "?q=" + encodeURIComponent(state.query) + hashAppend;
              }

              return renderItem(
                item,
                createElement,
                state,
                setActiveItemId,
                setContext,
                refresh,
                quartoSearchOptions
              );
            },
          },
        },
      ];
    },
  });

  window.quartoOpenSearch = () => {
    setIsOpen(false);
    setIsOpen(true);
    focusSearchInput();
  };

  document.addEventListener("keyup", (event) => {
    const { key } = event;
    const kbds = quartoSearchOptions["keyboard-shortcut"];
    const focusedEl = document.activeElement;

    const isFormElFocused = [
      "input",
      "select",
      "textarea",
      "button",
      "option",
    ].find((tag) => {
      return focusedEl.tagName.toLowerCase() === tag;
    });

    if (
      kbds &&
      kbds.includes(key) &&
      !isFormElFocused &&
      !document.activeElement.isContentEditable
    ) {
      event.preventDefault();
      window.quartoOpenSearch();
    }
  });

  // Remove the labeleledby attribute since it is pointing
  // to a non-existent label
  if (quartoSearchOptions.type === "overlay") {
    const inputEl = window.document.querySelector(
      "#quarto-search .aa-Autocomplete"
    );
    if (inputEl) {
      inputEl.removeAttribute("aria-labelledby");
    }
  }

  function throttle(func, wait) {
    let waiting = false;
    return function () {
      if (!waiting) {
        func.apply(this, arguments);
        waiting = true;
        setTimeout(function () {
          waiting = false;
        }, wait);
      }
    };
  }

  // If the main document scrolls dismiss the search results
  // (otherwise, since they're floating in the document they can scroll with the document)
  window.document.body.onscroll = throttle(() => {
    // Only do this if we're not detached
    // Bug #7117
    // This will happen when the keyboard is shown on ios (resulting in a scroll)
    // which then closed the search UI
    if (!window.matchMedia(detachedMediaQuery).matches) {
      setIsOpen(false);
    }
  }, 50);

  if (showSearchResults) {
    setIsOpen(true);
    focusSearchInput();
  }

  // Warm the engine when the reader shows intent, so the first keystroke
  // does not pay for the module + wasm + metadata fetches.
  searchEl.addEventListener(
    "focusin",
    () => {
      if (!quartoSearchOptions.algolia) {
        loadPagefind(quartoSearchOptions).catch(() => {});
      }
    },
    { once: true }
  );
});

function configurePlugins(quartoSearchOptions) {
  const autocompletePlugins = [];
  const algoliaOptions = quartoSearchOptions.algolia;
  if (
    algoliaOptions &&
    algoliaOptions["analytics-events"] &&
    algoliaOptions["search-only-api-key"] &&
    algoliaOptions["application-id"]
  ) {
    const apiKey = algoliaOptions["search-only-api-key"];
    const appId = algoliaOptions["application-id"];

    // Aloglia insights may not be loaded because they require cookie consent
    // Use deferred loading so events will start being recorded when/if consent
    // is granted.
    const algoliaInsightsDeferredPlugin = deferredLoadPlugin(() => {
      if (
        window.aa &&
        window["@algolia/autocomplete-plugin-algolia-insights"]
      ) {
        // Check if cookie consent is enabled from search options
        const cookieConsentEnabled =
          algoliaOptions["cookie-consent-enabled"] || false;

        // Generate random session token only when cookies are disabled
        const userToken = cookieConsentEnabled
          ? undefined
          : Array.from(Array(20), () =>
              Math.floor(Math.random() * 36).toString(36)
            ).join("");

        window.aa("init", {
          appId,
          apiKey,
          useCookie: cookieConsentEnabled,
          userToken: userToken,
        });

        const { createAlgoliaInsightsPlugin } =
          window["@algolia/autocomplete-plugin-algolia-insights"];
        // Register the insights client
        const algoliaInsightsPlugin = createAlgoliaInsightsPlugin({
          insightsClient: window.aa,
          onItemsChange({ insights, insightsEvents }) {
            const events = insightsEvents.flatMap((event) => {
              // This API limits the number of items per event to 20
              const chunkSize = 20;
              const itemChunks = [];
              const eventItems = event.items;
              for (let i = 0; i < eventItems.length; i += chunkSize) {
                itemChunks.push(eventItems.slice(i, i + chunkSize));
              }
              // Split the items into multiple events that can be sent
              const events = itemChunks.map((items) => {
                return {
                  ...event,
                  items,
                };
              });
              return events;
            });

            for (const event of events) {
              insights.viewedObjectIDs(event);
            }
          },
        });
        return algoliaInsightsPlugin;
      }
    });

    // Add the plugin
    autocompletePlugins.push(algoliaInsightsDeferredPlugin);
    return autocompletePlugins;
  }
}

// For plugins that may not load immediately, create a wrapper
// plugin and forward events and plugin data once the plugin
// is initialized. This is useful for cases like cookie consent
// which may prevent the analytics insights event plugin from initializing
// immediately.
function deferredLoadPlugin(createPlugin) {
  let plugin = undefined;
  let subscribeObj = undefined;
  const wrappedPlugin = () => {
    if (!plugin && subscribeObj) {
      plugin = createPlugin();
      if (plugin && plugin.subscribe) {
        plugin.subscribe(subscribeObj);
      }
    }
    return plugin;
  };

  return {
    subscribe: (obj) => {
      subscribeObj = obj;
    },
    onStateChange: (obj) => {
      const plugin = wrappedPlugin();
      if (plugin && plugin.onStateChange) {
        plugin.onStateChange(obj);
      }
    },
    onSubmit: (obj) => {
      const plugin = wrappedPlugin();
      if (plugin && plugin.onSubmit) {
        plugin.onSubmit(obj);
      }
    },
    onReset: (obj) => {
      const plugin = wrappedPlugin();
      if (plugin && plugin.onReset) {
        plugin.onReset(obj);
      }
    },
    getSources: (obj) => {
      const plugin = wrappedPlugin();
      if (plugin && plugin.getSources) {
        return plugin.getSources(obj);
      } else {
        return Promise.resolve([]);
      }
    },
    data: (obj) => {
      const plugin = wrappedPlugin();
      if (plugin && plugin.data) {
        plugin.data(obj);
      }
    },
  };
}

function validateItems(items) {
  // Validate the first item
  if (items.length > 0) {
    const item = items[0];
    const missingFields = [];
    if (item.type === kItemTypeError) {
      return;
    }
    if (item.href == undefined) {
      missingFields.push("href");
    }
    if (item.title == undefined) {
      missingFields.push("title");
    }
    if (item.text == undefined) {
      missingFields.push("text");
    }

    if (missingFields.length === 1) {
      throw {
        name: `Error: Search index is missing the <code>${missingFields[0]}</code> field.`,
        message: `The items being returned for this search do not include all the required fields. Please ensure that your index items include the <code>${missingFields[0]}</code> field or use <code>index-fields</code> in your <code>_quarto.yml</code> file to specify the field names.`,
      };
    } else if (missingFields.length > 1) {
      const missingFieldList = missingFields
        .map((field) => {
          return `<code>${field}</code>`;
        })
        .join(", ");

      throw {
        name: `Error: Search index is missing the following fields: ${missingFieldList}.`,
        message: `The items being returned for this search do not include all the required fields. Please ensure that your index items includes the following fields: ${missingFieldList}, or use <code>index-fields</code> in your <code>_quarto.yml</code> file to specify the field names.`,
      };
    }
  }
}

let lastQuery = null;
function showCopyLink(query, options) {
  const language = options.language;
  lastQuery = query;
  // Insert share icon
  const inputSuffixEl = window.document.body.querySelector(
    ".aa-Form .aa-InputWrapperSuffix"
  );

  if (inputSuffixEl) {
    let copyButtonEl = window.document.body.querySelector(
      ".aa-Form .aa-InputWrapperSuffix .aa-CopyButton"
    );

    if (copyButtonEl === null) {
      copyButtonEl = window.document.createElement("button");
      copyButtonEl.setAttribute("class", "aa-CopyButton");
      copyButtonEl.setAttribute("type", "button");
      copyButtonEl.setAttribute("title", language["search-copy-link-title"]);
      copyButtonEl.onmousedown = (e) => {
        e.preventDefault();
        e.stopPropagation();
      };

      const linkIcon = "bi-clipboard";
      const checkIcon = "bi-check2";

      const shareIconEl = window.document.createElement("i");
      shareIconEl.setAttribute("class", `bi ${linkIcon}`);
      copyButtonEl.appendChild(shareIconEl);
      inputSuffixEl.prepend(copyButtonEl);

      const copyUrlText = () => {
        const copyUrl = new URL(window.location);
        copyUrl.searchParams.set(kQueryArg, lastQuery);
        copyUrl.searchParams.set(kResultsArg, "1");
        return copyUrl.toString();
      };
      const flashCopied = (button) => {
        const icon = button.querySelector("i.bi");
        icon.classList.add(checkIcon);
        icon.classList.remove(linkIcon);
        setTimeout(function () {
          icon.classList.remove(checkIcon);
          icon.classList.add(linkIcon);
        }, 1000);
      };

      if (window.ClipboardJS) {
        const clipboard = new window.ClipboardJS(".aa-CopyButton", {
          text: copyUrlText,
        });
        clipboard.on("success", function (e) {
          flashCopied(e.trigger);
        });
      } else {
        copyButtonEl.addEventListener("click", () => {
          const button = copyButtonEl;
          if (navigator.clipboard) {
            navigator.clipboard
              .writeText(copyUrlText())
              .then(() => flashCopied(button))
              .catch(() => {});
          }
        });
      }
    }

    // If there is a query, show the link icon
    if (copyButtonEl) {
      if (lastQuery && options["copy-button"]) {
        copyButtonEl.style.display = "flex";
      } else {
        copyButtonEl.style.display = "none";
      }
    }
  }
}

/* Search engine: Pagefind */

// One Pagefind instance per page, created on first use (or on focus,
// to warm it). This is the single construction site: `basePath` and
// `baseUrl` are always explicit — Pagefind's defaults assume the
// bundle lives at `/pagefind/`, which it does not here — and federated
// indexes are merged right after init so every search sees them.
let pagefindPromise = undefined;
// Sites merged into the instance, by their result URL prefix, so a
// result can say where it came from.
const federatedSites = [];

function loadPagefind(options) {
  if (pagefindPromise === undefined) {
    pagefindPromise = initPagefind(options).catch((error) => {
      // Let the next search try again rather than caching a failure.
      pagefindPromise = undefined;
      throw error;
    });
  }
  return pagefindPromise;
}

async function initPagefind(options) {
  if (window.location.protocol === "file:") {
    throw {
      name: "Search requires a web server",
      message:
        "Search cannot run from a <code>file://</code> URL. Serve the site with <code>quarto preview</code> or any web server.",
    };
  }
  const siteRoot = new URL(offsetURL(""), window.location.href);
  const bundleUrl = new URL(options.bundle, siteRoot);
  const pagefind = await import(new URL("pagefind.js", bundleUrl).href);
  await pagefind.options({
    basePath: bundleUrl.href,
    baseUrl: siteRoot.href,
    excerptLength: 30,
  });
  await pagefind.init();

  const sites = Array.isArray(options.federate) ? options.federate : [];
  if (sites.length > 0) {
    const ownManifest = await fetchManifest(
      new URL("../manifest.json", bundleUrl)
    );
    for (const site of sites) {
      await mergeFederatedSite(pagefind, site, ownManifest, siteRoot);
    }
  }
  return pagefind;
}

async function fetchManifest(url) {
  try {
    const response = await fetch(url);
    if (!response.ok) return undefined;
    return await response.json();
  } catch (_error) {
    return undefined;
  }
}

// Merge another site's index (plan: "Federated search across sites").
// A q2 site is found through its fixed `site_libs/quarto-search/`
// location and its manifest is checked for the same Pagefind version:
// merged indexes are decoded by this page's runtime, so a different
// version would fail at decode time rather than loudly. A non-q2 site
// gives an explicit `bundle` path and no manifest check is possible.
async function mergeFederatedSite(pagefind, site, ownManifest, siteRoot) {
  let siteUrl;
  try {
    siteUrl = new URL(site.url, siteRoot);
  } catch (_error) {
    console.warn(`quarto-search: ignoring federated site with bad url: ${site.url}`);
    return;
  }
  if (!siteUrl.pathname.endsWith("/")) {
    siteUrl.pathname = siteUrl.pathname + "/";
  }
  let title = site.title;
  let bundlePath;
  if (site.bundle) {
    bundlePath = new URL(site.bundle, siteUrl);
  } else {
    const manifestUrl = new URL("site_libs/quarto-search/manifest.json", siteUrl);
    const manifest = await fetchManifest(manifestUrl);
    if (!manifest) {
      console.warn(
        `quarto-search: federated site ${siteUrl.href} has no search manifest; skipping`
      );
      return;
    }
    if (
      ownManifest &&
      manifest.pagefind &&
      ownManifest.pagefind &&
      manifest.pagefind !== ownManifest.pagefind
    ) {
      console.warn(
        `quarto-search: federated site ${siteUrl.href} was indexed with Pagefind ${manifest.pagefind}; this site uses ${ownManifest.pagefind}. Skipping it until both are rendered with the same Quarto version.`
      );
      return;
    }
    title = title || manifest.title;
    bundlePath = new URL(manifest.bundle || "pagefind/", manifestUrl);
  }
  if (!bundlePath.pathname.endsWith("/")) {
    bundlePath.pathname = bundlePath.pathname + "/";
  }
  const mergeOptions = {
    baseUrl: siteUrl.href,
  };
  if (typeof site.weight === "number") {
    mergeOptions.indexWeight = site.weight;
  }
  if (title) {
    mergeOptions.mergeFilter = { site: title };
  }
  try {
    await pagefind.mergeIndex(bundlePath.href, mergeOptions);
    federatedSites.push({ prefix: siteUrl.href, title: title || siteUrl.host });
  } catch (error) {
    console.warn(`quarto-search: could not merge ${bundlePath.href}: ${error}`);
  }
}

function siteForUrl(url) {
  for (const site of federatedSites) {
    if (url.startsWith(site.prefix)) {
      return site.title;
    }
  }
  return undefined;
}

// Pagefind's excerpts mark matches with a bare <mark>; the stylesheet
// (and Quarto 1's Algolia path) use `mark.search-match`.
function classifyMarks(html) {
  return html ? html.replace(/<mark>/g, "<mark class='search-match'>") : html;
}

function parseCrumbs(raw) {
  if (!raw) return undefined;
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : undefined;
  } catch (_error) {
    return undefined;
  }
}

// Search with Pagefind and reshape its documents + sub-results into
// Quarto 1's flat item list (one item per document, one per matching
// section), which `reshape` above groups back by document. `limit`
// caps items, as it did in Quarto 1.
async function pagefindSearch(query, limit, options) {
  const pagefind = await loadPagefind(options);
  const response = await pagefind.search(query);
  const max = typeof limit === "number" && limit > 0 ? limit : 50;
  // Each document yields at least one item, so at most `max` documents
  // can matter; load no more than that.
  const results = response.results.slice(0, max);
  const pages = await Promise.all(results.map((result) => result.data()));

  const items = [];
  for (const page of pages) {
    const crumbs = parseCrumbs(page.meta && page.meta.crumbs);
    const site = siteForUrl(page.url);
    const title = (page.meta && page.meta.title) || page.url;
    items.push({
      title,
      section: "",
      href: page.url,
      text: classifyMarks(page.excerpt),
      crumbs,
      site,
    });
    for (const sub of page.sub_results || []) {
      // The first sub-result is the page itself (content before any
      // heading); the document item already covers it.
      if (!sub.url || sub.url === page.url || !sub.url.includes("#")) {
        continue;
      }
      items.push({
        title,
        section: sub.title,
        href: sub.url,
        text: classifyMarks(sub.excerpt),
        crumbs,
        site,
      });
    }
  }
  return items.slice(0, max);
}

function inputElement() {
  return window.document.body.querySelector(".aa-Form .aa-Input");
}

function focusSearchInput() {
  setTimeout(() => {
    const inputEl = inputElement();
    if (inputEl) {
      inputEl.focus();
    }
  }, 50);
}

/* Panels */
const kItemTypeDoc = "document";
const kItemTypeMore = "document-more";
const kItemTypeItem = "document-item";
const kItemTypeError = "error";

function renderItem(
  item,
  createElement,
  state,
  setActiveItemId,
  setContext,
  refresh,
  quartoSearchOptions
) {
  switch (item.type) {
    case kItemTypeDoc:
      return createDocumentCard(
        createElement,
        "file-richtext",
        item.title,
        item.section,
        item.text,
        item.href,
        item.crumbs,
        item.site,
        quartoSearchOptions
      );
    case kItemTypeMore:
      return createMoreCard(
        createElement,
        item,
        state,
        setActiveItemId,
        setContext,
        refresh
      );
    case kItemTypeItem:
      return createSectionCard(
        createElement,
        item.section,
        item.text,
        item.href
      );
    case kItemTypeError:
      return createErrorCard(createElement, item.title, item.text);
    default:
      return undefined;
  }
}

function createDocumentCard(
  createElement,
  icon,
  title,
  section,
  text,
  href,
  crumbs,
  site,
  quartoSearchOptions
) {
  const iconEl = createElement("i", {
    class: `bi bi-${icon} search-result-icon`,
  });
  const titleEl = createElement("p", { class: "search-result-title" }, title);
  const titleContents = [iconEl, titleEl];
  // Federated results say which site they come from.
  if (site) {
    titleContents.push(
      createElement("p", { class: "search-result-site" }, site)
    );
  }
  const showParent = quartoSearchOptions["show-item-context"];
  if (crumbs && showParent) {
    let crumbsOut = undefined;
    const crumbClz = ["search-result-crumbs"];
    if (showParent === "root") {
      crumbsOut = crumbs.length > 1 ? crumbs[0] : undefined;
    } else if (showParent === "parent") {
      crumbsOut = crumbs.length > 1 ? crumbs[crumbs.length - 2] : undefined;
    } else {
      crumbsOut = crumbs.length > 1 ? crumbs.join(" > ") : undefined;
      crumbClz.push("search-result-crumbs-wrap");
    }

    const crumbEl = createElement(
      "p",
      { class: crumbClz.join(" ") },
      crumbsOut
    );
    titleContents.push(crumbEl);
  }

  const titleContainerEl = createElement(
    "div",
    { class: "search-result-title-container" },
    titleContents
  );

  const textEls = [];
  if (section) {
    const sectionEl = createElement(
      "p",
      { class: "search-result-section" },
      section
    );
    textEls.push(sectionEl);
  }
  const descEl = createElement("p", {
    class: "search-result-text",
    dangerouslySetInnerHTML: {
      __html: text,
    },
  });
  textEls.push(descEl);

  const textContainerEl = createElement(
    "div",
    { class: "search-result-text-container" },
    textEls
  );

  const containerEl = createElement(
    "div",
    {
      class: "search-result-container",
    },
    [titleContainerEl, textContainerEl]
  );

  const linkEl = createElement(
    "a",
    {
      href: offsetURL(href),
      class: "search-result-link",
    },
    containerEl
  );

  const classes = ["search-result-doc", "search-item"];
  if (!section) {
    classes.push("document-selectable");
  }

  return createElement(
    "div",
    {
      class: classes.join(" "),
    },
    linkEl
  );
}

function createMoreCard(
  createElement,
  item,
  state,
  setActiveItemId,
  setContext,
  refresh
) {
  const moreCardEl = createElement(
    "div",
    {
      class: "search-result-more search-item",
      onClick: (e) => {
        // Handle expanding the sections by adding the expanded
        // section to the list of expanded sections
        toggleExpanded(item, state, setContext, setActiveItemId, refresh);
        e.stopPropagation();
      },
    },
    item.title
  );

  return moreCardEl;
}

function toggleExpanded(item, state, setContext, setActiveItemId, refresh) {
  const expanded = state.context.expanded || [];
  if (expanded.includes(item.target)) {
    setContext({
      expanded: expanded.filter((target) => target !== item.target),
    });
  } else {
    setContext({ expanded: [...expanded, item.target] });
  }

  refresh();
  setActiveItemId(item.__autocomplete_id);
}

function createSectionCard(createElement, section, text, href) {
  const sectionEl = createSection(createElement, section, text, href);
  return createElement(
    "div",
    {
      class: "search-result-doc-section search-item",
    },
    sectionEl
  );
}

function createSection(createElement, title, text, href) {
  const descEl = createElement("p", {
    class: "search-result-text",
    dangerouslySetInnerHTML: {
      __html: text,
    },
  });

  const titleEl = createElement("p", { class: "search-result-section" }, title);
  const linkEl = createElement(
    "a",
    {
      href: offsetURL(href),
      class: "search-result-link",
    },
    [titleEl, descEl]
  );
  return linkEl;
}

function createErrorCard(createElement, title, text) {
  const descEl = createElement("p", {
    class: "search-error-text",
    dangerouslySetInnerHTML: {
      __html: text,
    },
  });

  const titleEl = createElement("p", {
    class: "search-error-title",
    dangerouslySetInnerHTML: {
      __html: `<i class="bi bi-exclamation-circle search-error-icon"></i> ${title}`,
    },
  });
  const errorEl = createElement("div", { class: "search-error" }, [
    titleEl,
    descEl,
  ]);
  return errorEl;
}

function positionPanel(_pos) {
  const panelEl = window.document.querySelector(
    "#quarto-search-results .aa-Panel"
  );
  const inputEl = window.document.querySelector(
    "#quarto-search .aa-Autocomplete"
  );

  if (panelEl && inputEl) {
    panelEl.style.top = `${Math.round(panelEl.offsetTop)}px`;
  }
}

/* Highlighting */
// highlighting functions
function highlightMatch(query, text) {
  if (text) {
    const start = text.toLowerCase().indexOf(query.toLowerCase());
    if (start !== -1) {
      const startMark = "<mark class='search-match'>";
      const endMark = "</mark>";

      const end = start + query.length;
      text =
        text.slice(0, start) +
        startMark +
        text.slice(start, end) +
        endMark +
        text.slice(end);
      const startInfo = clipStart(text, start);
      const endInfo = clipEnd(
        text,
        startInfo.position + startMark.length + endMark.length
      );
      text =
        startInfo.prefix +
        text.slice(startInfo.position, endInfo.position) +
        endInfo.suffix;

      return text;
    } else {
      return text;
    }
  } else {
    return text;
  }
}

function clipStart(text, pos) {
  const clipStart = pos - 50;
  if (clipStart < 0) {
    // This will just return the start of the string
    return {
      position: 0,
      prefix: "",
    };
  } else {
    // We're clipping before the start of the string, walk backwards to the first space.
    const spacePos = findSpace(text, pos, -1);
    return {
      position: spacePos.position,
      prefix: "",
    };
  }
}

function clipEnd(text, pos) {
  const clipEnd = pos + 200;
  if (clipEnd > text.length) {
    return {
      position: text.length,
      suffix: "",
    };
  } else {
    const spacePos = findSpace(text, clipEnd, 1);
    return {
      position: spacePos.position,
      suffix: spacePos.clipped ? "…" : "",
    };
  }
}

function findSpace(text, start, step) {
  let stepPos = start;
  while (stepPos > -1 && stepPos < text.length) {
    const char = text[stepPos];
    if (char === " " || char === "," || char === ":") {
      return {
        position: step === 1 ? stepPos : stepPos - step,
        clipped: stepPos > 1 && stepPos < text.length,
      };
    }
    stepPos = stepPos + step;
  }

  return {
    position: stepPos - step,
    clipped: false,
  };
}

// removes highlighting as implemented by the mark tag
function clearHighlight(searchterm, el) {
  const childNodes = el.childNodes;
  for (let i = childNodes.length - 1; i >= 0; i--) {
    const node = childNodes[i];
    if (node.nodeType === Node.ELEMENT_NODE) {
      if (
        node.tagName === "MARK" &&
        node.innerText.toLowerCase() === searchterm.toLowerCase()
      ) {
        el.replaceChild(document.createTextNode(node.innerText), node);
      } else {
        clearHighlight(searchterm, node);
      }
    }
  }
}

/** Get all html nodes under the given `root` that don't have children. */
function getLeafNodes(root) {
  let leaves = [];

  function traverse(node) {
    if (node.childNodes.length === 0) {
      leaves.push(node);
    } else {
      node.childNodes.forEach(traverse);
    }
  }

  traverse(root);
  return leaves;
}
/** create and return `<mark>${txt}</mark>` */
const markEl = (txt) => {
  const el = document.createElement("mark");
  el.appendChild(document.createTextNode(txt));
  return el;
};
/** get all ancestors of an element matching the given css selector */
const matchAncestors = (el, selector) => {
  let ancestors = [];
  while (el) {
    if (el.matches?.(selector)) ancestors.push(el);
    el = el.parentNode;
  }
  return ancestors;
};

const isWhitespace = (s) => s.trim().length === 0;
// =================
// MATCHING CODE
// =================
const initMatch = () => ({
  i: 0,
  lohisByNode: new Map(),
});
/**
 * keeps track of the start (lo) and end (hi) index of the match per node (leaf)
 * note: mutates the contents of `matchContext`
 */
const advanceMatch = (leaf, leafi, matchContext) => {
  matchContext.i++;

  const curLoHi = matchContext.lohisByNode.get(leaf);

  matchContext.lohisByNode.set(leaf, { lo: curLoHi?.lo ?? leafi, hi: leafi });
};
/**
 * Finds all non-overlapping matches for a search string in the document.
 * The search string may be split between multiple consecutive leaf nodes.
 *
 * Whitespace in the search string must be present in the document to match, but
 * there may be addititional whitespace in the document that is ignored.
 *
 * e.g. searching for `dogs rock` would match `dogs  \n <span> rock</span>`,
 * and would contribute the match
 * `{ i:9, els: new Map([[textNode, {lo:0, hi:8}],[spanNode,{lo:0,hi:5}]]) }`
 *
 * @returns {Map<HTMLElement,{lo:number,hi:number}>[]}
 */
function searchMatches(inSearch, el) {
  // searchText has all sequences of whitespace replaced by a single space
  const searchText = inSearch.toLowerCase().replace(/\s+/g, " ");
  const leafNodes = getLeafNodes(el);

  /** @type {Map<HTMLElement,{lo:number,hi:number}>[]} */
  const matches = [];
  /** @type {{i:number; els:Map<HTMLElement,{lo:number,hi:number}>}[]} */
  let curMatchContext = initMatch();

  for (const leaf of leafNodes) {
    const leafStr = leaf.textContent.toLowerCase();
    // for each character in this leaf's text:
    for (let leafi = 0; leafi < leafStr.length; leafi++) {
      if (isWhitespace(leafStr[leafi])) {
        // if there is at least one whitespace in the document
        // we advance over a search text whitespace.
        if (isWhitespace(searchText[curMatchContext.i]))
          advanceMatch(leaf, leafi, curMatchContext);
        // all sequences of whitespace are otherwise ignored.
      } else {
        if (searchText[curMatchContext.i] === leafStr[leafi]) {
          advanceMatch(leaf, leafi, curMatchContext);
        } else {
          curMatchContext = initMatch();
          // if current character in the document did not match at i in the search text,
          // reset the search and see if that character matches at 0 in the search text.
          if (searchText[curMatchContext.i] === leafStr[leafi])
            advanceMatch(leaf, leafi, curMatchContext);
        }
      }

      const isMatchComplete = curMatchContext.i === searchText.length;
      if (isMatchComplete) {
        matches.push(curMatchContext.lohisByNode);
        curMatchContext = initMatch();
      }
    }
  }

  return matches;
}

/**
 * e.g. `markMatches(myTextNode, [[0,5],[12,15]])` would wrap the
 * character sequences in myTextNode from 0-5 and 12-15 in marks.
 * Its important to mark all sequences in a text node at once
 * because this function replaces the entire text node; so any
 * other references to that text node will no longer be in the DOM.
 */
function markMatches(node, lohis) {
  const text = node.nodeValue;

  const markFragment = document.createDocumentFragment();

  let prevHi = 0;
  for (const [lo, hi] of lohis) {
    markFragment.append(
      document.createTextNode(text.slice(prevHi, lo)),
      markEl(text.slice(lo, hi + 1))
    );
    prevHi = hi + 1;
  }
  markFragment.append(
    document.createTextNode(text.slice(prevHi, text.length))
  );

  const parent = node.parentElement;
  parent?.replaceChild(markFragment, node);
  return parent;
}

// Activate ancestor tabs so a search match inside an inactive pane becomes visible.
// When multiple panes in the same tabset contain matches, avoid switching away from
// the currently active pane — the user already sees a match there.
function openAllTabsetsContainingEl(el) {
  for (const pane of matchAncestors(el, ".tab-pane")) {
    const tabContent = pane.closest(".tab-content");
    if (!tabContent) continue;
    const activePane = tabContent.querySelector(":scope > .tab-pane.active");
    if (activePane?.querySelector("mark")) continue;
    const tabButton = document.querySelector(`[data-bs-target="#${pane.id}"]`);
    if (tabButton && window.bootstrap) new bootstrap.Tab(tabButton).show();
  }
}

function scrollToFirstVisibleMatch(mainEl) {
  for (const mark of mainEl.querySelectorAll("mark")) {
    const isMarkVisible = matchAncestors(mark, ".tab-pane").every(
      (markTabPane) => markTabPane.classList.contains("active")
    );
    if (isMarkVisible) {
      mark.scrollIntoView({ behavior: "smooth", block: "center" });
      return;
    }
  }
}

/**
 * e.g.
 * ```js
 * const m = new Map()
 *
 * arrayMapPush(m, 'dog', 'Max')
 * console.log(m) // Map { dog->['Max'] }
 *
 * arrayMapPush(m, 'dog', 'Samba')
 * arrayMapPush(m, 'cat', 'Scruffle')
 * console.log(m) // Map { dog->['Max', 'Samba'], cat->['Scruffle'] }
 * ```
 */
const arrayMapPush = (map, key, item) => {
  if (!map.has(key)) map.set(key, []);
  map.set(key, [...map.get(key), item]);
};

// copy&paste any string from a quarto page and
// this should find that string in the page and highlight it.
// exception: text that starts outside/inside a tabset and ends
// inside/outside that tabset.
function highlight(searchStr, el) {
  const matches = searchMatches(searchStr, el);

  const matchesGroupedByNode = new Map();
  for (const match of matches) {
    for (const [mel, { lo, hi }] of match) {
      arrayMapPush(matchesGroupedByNode, mel, [lo, hi]);
    }
  }

  for (const [node, lohis] of matchesGroupedByNode) {
    markMatches(node, lohis);
  }
}

/* Link Handling */
// get the offset from this page for a given site root relative url.
// Absolute URLs (Pagefind results, federated sites) and root-relative
// URLs are returned unchanged.
function offsetURL(url) {
  if (/^(https?:)?\/\//.test(url) || url.startsWith("/")) {
    return url;
  }
  var offset = getMeta("quarto:offset");
  return offset ? offset + url : url;
}

// read a meta tag value
function getMeta(metaName) {
  var metas = window.document.getElementsByTagName("meta");
  for (let i = 0; i < metas.length; i++) {
    if (metas[i].getAttribute("name") === metaName) {
      return metas[i].getAttribute("content");
    }
  }
  return "";
}

function algoliaSearch(query, limit, algoliaOptions) {
  const { getAlgoliaResults } = window["@algolia/autocomplete-preset-algolia"];

  const applicationId = algoliaOptions["application-id"];
  const searchOnlyApiKey = algoliaOptions["search-only-api-key"];
  const indexName = algoliaOptions["index-name"];
  const indexFields = algoliaOptions["index-fields"];
  const searchClient = window.algoliasearch(applicationId, searchOnlyApiKey);
  const searchParams = algoliaOptions["params"];
  const searchAnalytics = !!algoliaOptions["analytics-events"];

  return getAlgoliaResults({
    searchClient,
    queries: [
      {
        indexName: indexName,
        query,
        params: {
          hitsPerPage: limit,
          clickAnalytics: searchAnalytics,
          ...searchParams,
        },
      },
    ],
    transformResponse: (response) => {
      if (!indexFields) {
        return response.hits.map((hit) => {
          return hit.map((item) => {
            return {
              ...item,
              text: highlightMatch(query, item.text),
            };
          });
        });
      } else {
        const remappedHits = response.hits.map((hit) => {
          return hit.map((item) => {
            const newItem = { ...item };
            ["href", "section", "title", "text", "crumbs"].forEach(
              (keyName) => {
                const mappedName = indexFields[keyName];
                if (
                  mappedName &&
                  item[mappedName] !== undefined &&
                  mappedName !== keyName
                ) {
                  newItem[keyName] = item[mappedName];
                  delete newItem[mappedName];
                }
              }
            );
            newItem.text = highlightMatch(query, newItem.text);
            return newItem;
          });
        });
        return remappedHits;
      }
    },
  });
}
