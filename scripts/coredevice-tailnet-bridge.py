#!/usr/bin/env python3
"""Temporary CoreDevice relay. No persistent config or pairing changes.

Run on the paired Mac with real captured Bonjour instance/TXT values.
Only this Mac may connect to the relay; payloads are never logged.
"""
import argparse
import asyncio
import signal


async def main(args):
    loop = asyncio.get_running_loop()
    stop = asyncio.Event()
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        loop.add_signal_handler(sig, stop.set)
    allowed = {args.bind, "127.0.0.1"}
    servers, datagrams, processes = [], [], []

    async def tcp(reader, writer, port):
        peer = writer.get_extra_info("peername")
        if peer[0] not in allowed:
            writer.close()
            return
        upstream = None
        try:
            remote_reader, upstream = await asyncio.wait_for(
                asyncio.open_connection(args.phone, port), 8)
            print("TCP connected port", port, flush=True)

            async def copy(src, dst):
                while True:
                    data = await src.read(65536)
                    if not data:
                        if dst.can_write_eof():
                            dst.write_eof()
                        return
                    dst.write(data)
                    await dst.drain()

            await asyncio.gather(copy(reader, upstream), copy(remote_reader, writer))
        except (OSError, asyncio.TimeoutError) as error:
            print("TCP error", port, str(error), flush=True)
        finally:
            writer.close()
            if upstream:
                upstream.close()

    class Reply(asyncio.DatagramProtocol):
        def __init__(self, listener, client):
            self.listener, self.client = listener, client

        def datagram_received(self, data, addr):
            self.listener.sendto(data, self.client)

    class UDP(asyncio.DatagramProtocol):
        def __init__(self, port):
            self.port, self.peers, self.tasks = port, {}, set()

        def connection_made(self, transport):
            self.transport = transport

        def datagram_received(self, data, client):
            if client[0] not in allowed:
                return
            task = asyncio.create_task(self.forward(data, client))
            self.tasks.add(task)
            task.add_done_callback(self.tasks.discard)

        async def forward(self, data, client):
            if client not in self.peers:
                # Store the task before awaiting so concurrent packets share one socket.
                self.peers[client] = asyncio.create_task(loop.create_datagram_endpoint(
                    lambda: Reply(self.transport, client),
                    remote_addr=(args.phone, self.port)))
                print("UDP connected port", self.port, flush=True)
            try:
                transport, _ = await self.peers[client]
                transport.sendto(data)
            except OSError as error:
                print("UDP error", self.port, str(error), flush=True)

        def connection_lost(self, error):
            for task in self.tasks:
                task.cancel()
            for task in self.peers.values():
                if task.done() and not task.cancelled() and task.exception() is None:
                    task.result()[0].close()
                else:
                    task.cancel()

    ports = {args.service_port}
    for item in args.ports.split(",") if args.ports else []:
        bounds = item.split("-")
        ports.update(range(int(bounds[0]), int(bounds[-1]) + 1))
    if len(ports) > 350 or any(p < 1024 or p > 65535 for p in ports):
        raise ValueError("Use at most 350 explicit unprivileged ports")
    try:
        for port in sorted(ports):
            servers.append(await asyncio.start_server(
                lambda r, w, p=port: tcp(r, w, p), args.bind, port))
            transport, _ = await loop.create_datagram_endpoint(
                lambda p=port: UDP(p), local_addr=(args.bind, port))
            datagrams.append(transport)
        print("Relay listening on", args.bind, "ports", sorted(ports), flush=True)
        proc = await asyncio.create_subprocess_exec(
            "/usr/bin/dns-sd", "-P", args.instance, "_remotepairing._tcp", "local",
            str(args.service_port), args.hostname, args.bind, *args.txt)
        processes.append(proc)
        waiters = [asyncio.create_task(stop.wait()), asyncio.create_task(proc.wait())]
        await asyncio.wait(waiters, return_when=asyncio.FIRST_COMPLETED)
        for task in waiters:
            task.cancel()
    finally:
        for proc in processes:
            if proc.returncode is None:
                proc.terminate()
                await proc.wait()
        for server in servers:
            server.close()
            await server.wait_closed()
        for transport in datagrams:
            transport.close()
        print("Bridge stopped", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", required=True)
    parser.add_argument("--phone", required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--hostname", required=True)
    parser.add_argument("--service-port", type=int, default=49152)
    parser.add_argument("--ports", default="")
    parser.add_argument("--txt", action="append", default=[])
    asyncio.run(main(parser.parse_args()))
