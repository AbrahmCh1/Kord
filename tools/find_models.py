# Finds 3D models of headphones on Sketchfab that Kord may use, by brand.
#
# Only downloadable models under licenses compatible with an MIT app are
# listed: CC Attribution and CC0 by default (add CC BY-SA with --share-alike;
# renders of those must then be shared under CC BY-SA too). NonCommercial,
# NoDerivs, "Standard" and "Editorial" models are never listed.
#
# Searching needs no account. Downloading does: create an API token at
# https://sketchfab.com/settings/password and put it in SKETCHFAB_TOKEN.
#
# Usage:
#   python tools/find_models.py                       # every brand in BRANDS
#   python tools/find_models.py --brand sony samsung  # some brands
#   python tools/find_models.py "nothing ear"         # free-form searches
#   python tools/find_models.py --report assets/model/candidates.md
#   python tools/find_models.py --download <uid> --to assets/model/GalaxyBuds2
#
# Then render the model with tools/render_model.py (see its header) and add
# the credit line saved next to the .glb to crates/kord/assets/sprites/CREDITS.md.
import argparse
import json
import os
import re
import sys
import time
import urllib.parse
import urllib.request

API = "https://api.sketchfab.com/v3"

# What to search for each brand: one query per product line.
BRANDS = {
    "apple": ["airpods pro", "airpods max", "airpods 4", "airpods"],
    "beats": ["powerbeats pro", "beats studio buds", "beats fit pro", "beats solo", "beats studio"],
    "samsung": ["galaxy buds"],
    "sony": ["wf-1000xm", "wh-1000xm", "sony earbuds", "sony headphones"],
    "bose": ["quietcomfort earbuds", "bose quietcomfort", "bose headphones"],
    "google": ["pixel buds"],
    "jbl": ["jbl earbuds", "jbl headphones"],
    "jabra": ["jabra elite"],
    "sennheiser": ["sennheiser momentum", "sennheiser headphones"],
    "nothing": ["nothing ear"],
    "xiaomi": ["redmi buds", "xiaomi earbuds"],
    "huawei": ["freebuds"],
    "oneplus": ["oneplus buds"],
}

LICENSES = {"by": "CC BY 4.0", "cc0": "CC0", "by-sa": "CC BY-SA 4.0"}
LICENSE_URLS = {
    "by": "http://creativecommons.org/licenses/by/4.0/",
    "cc0": "http://creativecommons.org/publicdomain/zero/1.0/",
    "by-sa": "http://creativecommons.org/licenses/by-sa/4.0/",
}


def get(url, token=None):
    request = urllib.request.Request(url, headers={"User-Agent": "kord-find-models"})
    if token:
        request.add_header("Authorization", f"Token {token}")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def search(query, license, count):
    params = {
        "type": "models",
        "q": query,
        "downloadable": "true",
        "license": license,
        "count": min(count, 24),
        "sort_by": "-likeCount",
    }
    results = []
    url = f"{API}/search?{urllib.parse.urlencode(params)}"
    while url and len(results) < count:
        page = get(url)
        results.extend(page.get("results", []))
        url = page.get("next")
        time.sleep(0.3)
    return results[:count]


# Something you wear on your ears must be named or tagged.
HEADPHONE = re.compile(
    r"\b(headphones?|headsets?|earbuds?|earphones?|buds|airpods|freebuds|beats|quietcomfort|qc\d+|"
    r"wh 1000\w*|wf 1000\w*|momentum|ear)\b"
)
# Other gadgets of the same brands, and the "free download" spam Sketchfab is full of.
UNWANTED = re.compile(r"\b(download|crack|for (mac|pc)|nsp|watch|phone|flip|fold|walkman|tripod|galaxy s\d+)\b")
# Fewer faces than this is a picture on a plane, not a model.
MIN_FACES = 500


def words(text):
    return re.sub(r"[^a-z0-9]+", " ", text.lower()).strip()


def describe(model):
    return words(" ".join([model.get("name") or ""] + [t.get("name", "") for t in model.get("tags", [])]))


def relevant(model, query):
    """Headphones whose name or tags mention every word of the query."""
    text = describe(model)
    glb = (model.get("archives") or {}).get("glb") or {}
    return (
        all(word in text for word in words(query).split())
        and HEADPHONE.search(text) is not None
        and UNWANTED.search(words(model.get("name") or "")) is None
        and (glb.get("faceCount") or model.get("faceCount") or 0) >= MIN_FACES
    )


def named_after(model, query):
    """The query is in the model's name itself, the strongest sign of a match."""
    return all(word in words(model["name"]) for word in words(query).split())


def summarize(model, license, query):
    glb = (model.get("archives") or {}).get("glb") or {}
    user = model.get("user") or {}
    return {
        "uid": model["uid"],
        "name": (model.get("name") or "").strip(),
        "author": user.get("displayName") or user.get("username") or "?",
        "author_url": user.get("profileUrl", ""),
        "license": license,
        "likes": model.get("likeCount", 0),
        "faces": glb.get("faceCount") or model.get("faceCount") or 0,
        "glb_mb": round((glb.get("size") or 0) / 1e6, 1),
        "url": f"https://sketchfab.com/3d-models/{model['uid']}",
        "thumbnail": max((model.get("thumbnails") or {}).get("images", []), key=lambda i: i.get("width", 0), default={}).get("url", ""),
        "query": query,
    }


def credit(model):
    """The attribution line, in the format of assets/model/*/…cc.txt."""
    return (
        f'"{model["name"]}" ({model["url"]}) by {model["author"]} is licensed under '
        f'{LICENSES[model["license"]]} ({LICENSE_URLS[model["license"]]}).'
    )


def find(groups, licenses, count, strict):
    found = {}
    for group, queries in groups.items():
        seen = set()
        models = []
        for query in queries:
            for license in licenses:
                try:
                    results = search(query, license, count)
                except Exception as err:  # keep going with the other searches
                    print(f"  ! {query} ({license}): {err}", file=sys.stderr)
                    continue
                for model in results:
                    if model["uid"] in seen or (strict and not relevant(model, query)):
                        continue
                    seen.add(model["uid"])
                    models.append(summarize(model, license, query))
        models.sort(key=lambda m: (not named_after(m, m["query"]), -m["likes"]))
        found[group] = models
        print(f"{group}: {len(models)} models", file=sys.stderr)
    return found


def print_table(found):
    # Model names have emoji; Windows consoles default to a legacy code page.
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")
    for group, models in found.items():
        print(f"\n== {group} ({len(models)}) ==")
        for m in models:
            print(f"  {m['likes']:>4} likes  {m['name'][:44]:<44}  {m['author'][:18]:<18}  {LICENSES[m['license']]:<12}  "
                  f"{m['faces']:>7} faces  {m['glb_mb']:>5} MB  {m['uid']}")


def write_report(found, path):
    lines = [
        "# 3D model candidates",
        "",
        "Downloadable headphone models on Sketchfab under licenses Kord can use, found by",
        "`tools/find_models.py`. Check each model's page before using it: licenses can",
        "change, and some uploads are copies of someone else's work.",
        "",
    ]
    for group, models in found.items():
        lines += [f"## {group.capitalize()}", ""]
        if not models:
            lines += ["No matching models.", ""]
            continue
        lines += ["| Model | Author | License | Likes | Faces | GLB | uid |", "|---|---|---|---|---|---|---|"]
        for m in models:
            name = m["name"].replace("|", "/") or "(untitled)"
            lines.append(
                f"| [{name}]({m['url']}) | [{m['author']}]({m['author_url']}) | {LICENSES[m['license']]} | "
                f"{m['likes']} | {m['faces']:,} | {m['glb_mb']} MB | `{m['uid']}` |"
            )
        lines.append("")
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    print(f"wrote {path}", file=sys.stderr)


def download(uid, directory, token, name=None):
    model = get(f"{API}/models/{uid}")
    slug = (model.get("license") or {}).get("slug", "")
    if slug not in LICENSES:
        sys.exit(f"{uid} is licensed '{slug}', which Kord can't use")
    links = get(f"{API}/models/{uid}/download", token)
    glb = links.get("glb")
    if not glb:
        sys.exit(f"{uid} has no .glb download (formats: {', '.join(links)})")
    name = name or re.sub(r"[^a-z0-9]+", "-", (model.get("name") or uid).lower()).strip("-") or uid
    os.makedirs(directory, exist_ok=True)
    target = os.path.join(directory, f"{name}.glb")
    urllib.request.urlretrieve(glb["url"], target)
    summary = summarize(model, slug, "")
    with open(os.path.join(directory, f"{name}cc.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write(credit(summary) + "\n")
    print(f"saved {target} ({os.path.getsize(target) / 1e6:.1f} MB)")
    print(credit(summary))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("queries", nargs="*", help="free-form searches (default: every brand)")
    parser.add_argument("--brand", nargs="+", choices=sorted(BRANDS), help="search these brands")
    parser.add_argument("--count", type=int, default=24, help="results per search and license")
    parser.add_argument("--share-alike", action="store_true", help="also list CC BY-SA models")
    parser.add_argument("--loose", action="store_true", help="keep results that don't mention every query word")
    parser.add_argument("--report", help="write a Markdown table of the results to this file")
    parser.add_argument("--json", help="write the results as JSON to this file")
    parser.add_argument("--download", metavar="UID", help="download one model's .glb (needs SKETCHFAB_TOKEN)")
    parser.add_argument("--to", default="assets/model", help="where --download saves the model")
    parser.add_argument("--name", help="file name for --download (default: from the model name)")
    opts = parser.parse_args()

    if opts.download:
        token = os.environ.get("SKETCHFAB_TOKEN")
        if not token:
            sys.exit("set SKETCHFAB_TOKEN to your Sketchfab API token (sketchfab.com/settings/password)")
        download(opts.download, opts.to, token, opts.name)
        return

    if opts.queries:
        groups = {q: [q] for q in opts.queries}
    else:
        groups = {b: BRANDS[b] for b in (opts.brand or BRANDS)}
    licenses = ["by", "cc0"] + (["by-sa"] if opts.share_alike else [])
    found = find(groups, licenses, opts.count, strict=not opts.loose)
    print_table(found)
    if opts.report:
        write_report(found, opts.report)
    if opts.json:
        with open(opts.json, "w", encoding="utf-8") as f:
            json.dump(found, f, indent=2, ensure_ascii=False)


if __name__ == "__main__":
    main()
