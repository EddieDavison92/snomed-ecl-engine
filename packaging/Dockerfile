# The release's Linux executables, with term matching, in a small image.
# The release workflow places each architecture's executable at
# dist/docker/<arch>/snomed-ecl-engine before building for both.
FROM debian:bookworm-slim
ARG TARGETARCH
COPY dist/docker/${TARGETARCH}/snomed-ecl-engine /usr/local/bin/snomed-ecl-engine
WORKDIR /data
ENTRYPOINT ["snomed-ecl-engine"]
