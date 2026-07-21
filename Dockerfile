# Start with the official Rust image
FROM rust:1.85.1-slim AS builder

# Install dependencies needed for Bevy and your project
RUN apt-get update && \
    apt-get install -y \
    libwayland-dev \
    libasound2-dev \
    libudev-dev \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory
WORKDIR /usr/src/app

# Copy the rest of your source code to the container
COPY . .

# Build your project
RUN cargo build --release --features server

# Now we’ll build the runtime image
FROM debian:bookworm-slim

# Install the necessary libraries to run Bevy and your project
RUN apt-get update && \
    apt-get install -y \
    libwayland-client0 \
    libasound2 \
    libudev1 \
    libssl-dev \
    libxcb1 \
    && rm -rf /var/lib/apt/lists/*

# Set up the environment to ensure dynamic libraries are found
ENV LD_LIBRARY_PATH="/usr/local/lib:$LD_LIBRARY_PATH"
ENV WGPU_FORCE_HEADLESS=1

# Create a directory for the server
WORKDIR /usr/local/bin

# Copy the built server from the builder container
COPY --from=builder /usr/src/app/target/release/chexy-butt-balloons /usr/local/bin/chexy-butt-balloons

# Optionally copy static data if the server needs it at runtime
COPY --from=builder /usr/src/app/assets /usr/local/share/chexy-butt-balloons/assets

# Expose the required ports (native, web transport, websocket, and auth HTTP)
EXPOSE 8080 8081 8082 8083

# Command to run the server
CMD ["./chexy-butt-balloons"]
