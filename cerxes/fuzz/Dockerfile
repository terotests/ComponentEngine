# SPDX-License-Identifier: AGPL-3.0-or-later
#
# cerxes-fuzz in a container, for a cloud VM or a container service:
#
#   docker build -f cerxes/fuzz/Dockerfile -t cerxes-fuzz .        (repo root)
#   docker run --rm -v $PWD/fuzz-out:/out -e MINUTES=240 -e JOBS=16 cerxes-fuzz
#
# Findings land in /out/<class>/; /out/summary.txt counts them.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cd cerxes/fuzz && cargo build --profile checked

FROM node:22-bookworm-slim
COPY --from=build /src/cerxes/fuzz/target/checked/cerxes-fuzz /usr/local/bin/cerxes-fuzz
COPY --from=build /src/cerxes/fuzz/known/known.txt /corpus/known.txt
COPY --from=build /src/cerxes/fuzz/seeds /corpus/seeds
COPY --from=build /src/cerxes/tests/cases.txt /corpus/cerxes-cases.txt
COPY --from=build /src/cer/tests/cases.txt /corpus/cer-cases.txt
ENV MINUTES=60 JOBS=0 EXTRA=""
VOLUME /out
CMD j=$JOBS; [ "$j" = 0 ] && j=$(nproc); \
    exec cerxes-fuzz run --time $((MINUTES * 60)) --jobs $j --out /out \
      --corpus /corpus/cerxes-cases.txt --corpus /corpus/cer-cases.txt --corpus /corpus/seeds \
      --known /corpus/known.txt $EXTRA
