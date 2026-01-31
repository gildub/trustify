# Using Marmaid Command Line to (re)generate images

Mermaid command line tool can be used export images out of Mermaid.

Detailed installation instructions are available on the Mermaid CLI repo, available at https://github.com/mermaid-js/mermaid-cli.
Alternatively to avoid installing the Node stack dependencies on your machine, a pre-built Docker image is available to use, which is documented in the Mermaid GitHub repo too.

The following command ingests the Marmaid markup and generate the PNG (SVG, PDF also supported).

```bash
mmdc -i domain-model.mmd -o domain-model.png
```

### Note

A markup file (md) can be provided too, in which case `mmdc` will extract all the Mermaid diagrams and save them in indivual image files.
Each file will be suffixed with an incremental number, i.e domain-model-1.png, domain-model-2.png, etc.

## Defaults values in `mermaid.json`

In order to generate or regenerate consistent diagrams we can use a configuration file for Mermaid options such as `--theme`.

For example :

```bash
mmdc -c mermaid.json -i domain-model.md -o domain-model.png
```

## Data model options

The data model being large, the following was used to generate a readable image :

```bash
mmdc -c mermaid.json -i data-model.md -o data-model.png --width 1200 --height 1600 --scale 6
```
