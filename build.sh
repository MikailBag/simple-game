#!/usr/bin/env bash
version="v1.0.0"
docker build -f Cargo.toml -t mikailbag/game-server:$version .
docker push mikailbag/game-server:$version