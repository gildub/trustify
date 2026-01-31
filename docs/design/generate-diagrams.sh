#!/usr/bin/env bash
mmdc -c mermaid.json -i domain-model.md -o domain-model.png
mmdc -c mermaid.json -i data-model.md -o data-model.png --width 1200 --height 1600 --scale 6
