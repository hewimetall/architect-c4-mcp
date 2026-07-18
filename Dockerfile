# syntax=docker/dockerfile:1
FROM python:3.12-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:/root/.local/bin:${PATH}"

RUN curl -LsSf https://astral.sh/uv/install.sh | sh

WORKDIR /app
COPY . .

RUN uv sync --extra dev \
 && uv run maturin develop --release --manifest-path packages/architect-c4-app/Cargo.toml

ENV ARCHITECT_C4_TRANSPORT=http \
    ARCHITECT_C4_HOST=0.0.0.0 \
    ARCHITECT_C4_PORT=8766 \
    ARCHITECT_C4_DOCS=/docs

EXPOSE 8766
CMD ["uv", "run", "architect-c4"]
