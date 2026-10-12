"""The pictures on docs/python/ext/data.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import data
from rs_rich.ext.transform import Pipeline

# --8<-- [start:deploy]
deploy = data.parse("""\
defaults: &defaults
  replicas: 2
  image: registry.example/app:1.4
services:
  web:
    port: 8080
    settings: *defaults
  worker: *defaults
""", "yaml")
# --8<-- [end:deploy]


def parse_error(console):
    # --8<-- [start:error]
    try:
        data.parse('{\n  "name": "api",\n  "port" 8080\n}\n', "json", name="api.json")
    except data.DataError as error:
        print(error.format, error.line, error.column)
        console.print(error.diagnostic)
    # --8<-- [end:error]


def views(console):
    # --8<-- [start:views]
    console.print(data.Explorer(deploy, root_label="deploy.yaml", fold=["defaults"]))
    console.print(data.TableView(
        [{"name": "web", "cpu": 0.42}, {"name": "db", "cpu": 0.91}],
        headers={"cpu": "CPU"},
    ))
    console.print(data.FlatView(data.parse('{"name": "api", "ports": [80, 443]}')))
    # --8<-- [end:views]


def searched(console):
    # --8<-- [start:search]
    for hit in data.search(deploy, key="port"):
        print(hit.path, hit.matched_on)
    console.print(data.SearchResults(deploy, path="services.**", value="8080"))

    api = data.parse('{"name": "api", "ports": [80, 443], "tls": true}')
    print(data.select(api, "$.ports[?(@ > 100)]")[0][0])
    new = data.parse('{"name": "api", "ports": [80, 8443], "tls": true, "hsts": true}')
    console.print(data.DataDiffView(api, new))
    # --8<-- [end:search]


def config(console):
    # --8<-- [start:config]
    ini = data.parse(
        "; production\n[database]\nhost = db.internal\npassword = hunter2\n[api]\ntimeout = 30\n",
        "ini",
    )
    console.print(data.ConfigFileView(ini, title="app.ini", redaction=data.Redaction.secrets()))
    # --8<-- [end:config]


def redacted(console):
    # --8<-- [start:pipeline]
    doc = data.parse('{"servers": [{"host": "a", "token": "t1"}, {"host": "b", "token": "t2"}]}')
    document = (
        Pipeline()
        .then("redact", data.Redact(data.Redaction.secrets()))
        .then("select", data.Select("$.servers"))
        .apply(data.Document(doc))
    )
    console.print(document)
    # --8<-- [end:pipeline]


shots = Shots("ext/data")
shots.shot("error", parse_error, width=60)
shots.shot("views", views, width=60)
shots.shot("search", searched, width=60)
shots.shot("config", config, width=60)
shots.shot("pipeline", redacted, width=60)
shots.finish()
