# Elduro web: Svelte frontend + Rust backend hub in one image.

FROM node:22-alpine AS frontend
WORKDIR /fe
COPY frontend/package.json ./
RUN npm install --no-fund --no-audit
COPY frontend/ ./
RUN npm run build

FROM rust:slim-bookworm AS backend
WORKDIR /app
COPY Cargo.toml ./
COPY backend ./backend
COPY capture ./capture
RUN cargo build --release -p elduro-backend

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=backend /app/target/release/elduro-backend /app/elduro-backend
COPY --from=frontend /fe/dist /app/static
ENV PORT=8080 STATIC_DIR=/app/static
EXPOSE 8080
USER 1000:1000
CMD ["/app/elduro-backend"]

