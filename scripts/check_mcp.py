"""Exercise the MCP server as a separate stdio process using scratch files."""

import argparse
import json
from pathlib import Path
from queue import Queue
import subprocess
from tempfile import TemporaryDirectory
from threading import Thread


def check(binary, directory, include_pdf):
    with subprocess.Popen(
        [str(binary)],
        cwd=directory,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        encoding="utf-8",
    ) as process:
        replies = Queue()
        def collect_replies():
            for line in process.stdout:
                replies.put(line)

        Thread(target=collect_replies, daemon=True).start()
        request_id = 0

        def send(message):
            process.stdin.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
            process.stdin.flush()

        def request(method, *, timeout=20, **params):
            nonlocal request_id
            request_id += 1
            send({"id": request_id, "method": method, "params": params})
            reply = json.loads(replies.get(timeout=timeout))
            assert reply["id"] == request_id, reply
            return reply.get("result", reply.get("error"))

        def tool(tool_name, *, error=None, timeout=20, **arguments):
            response = request("tools/call", timeout=timeout, name=tool_name, arguments=arguments)
            text = response["content"][0]["text"]
            if error:
                assert response["isError"] and error in text, response
                return
            assert not response["isError"], response
            return json.loads(text)

        try:
            assert request("server/discover")["code"] == -32601
            assert request("tools/list")["code"] == -32002
            init = request(
                "initialize",
                protocolVersion="2025-11-25",
                capabilities={},
                clientInfo={"name": "list-rs-smoke", "version": "1"},
            )
            assert init["protocolVersion"] == "2025-11-25"
            send({"method": "notifications/initialized"})
            assert request("ping") == {}
            assert len(request("tools/list")["tools"]) == 18
            assert len(request("resources/list")["resources"]) == 3

            tool("new_sheet", demo=True)
            tool("set_input", index=2, value=60)
            assert tool("get_region", index=6)["value"] == "1.5 MPa"
            assert tool("eval_expr", expression="p / MPa =", use_current_sheet=True)["value"] == "1.5"
            assert len(tool("list_regions")["regions"]) == 9
            xml_resource = request("resources/read", uri="sheet://current/xml")
            assert ">60<" in xml_resource["contents"][0]["text"]
            tool("new_sheet", error="discard_unsaved")
            tool("set_display_unit", index=6, unit="kPa")
            assert tool("get_region", index=6)["value"] == "1500 kPa"
            tool("reset_input", index=2)
            assert tool("get_region", index=6)["value"] == "1000 kPa"
            tool("set_input", index=2, value=60)
            tool("set_precision", precision=8)

            region = tool("insert_region", kind="formula", formula="x := 2")["region"]
            region_id = region["region_id"]
            tool("edit_formula", region_id=region_id, formula="x := 5")
            tool("move_region", region_id=region_id, left=25, top=450)
            assert tool("get_region", region_id=region_id)["value"] == "5"
            tool("delete_region", region_id=region_id)
            assert tool("import_csv", name="M", csv_text="1,2\n3,4")["rows"] == 2
            table = next(r for r in tool("list_regions")["regions"] if r["label"] == "M")
            assert "1,2" in tool("export_csv", region_id=table["region_id"])["csv"]
            assert tool("verify_snapshots")["failed"] == 0
            tool("export_report", path="report.html")
            assert "<html" in (directory / "report.html").read_text(encoding="utf-8")
            if include_pdf:
                tool("export_report", path="report.pdf", format="pdf", timeout=65)
                pdf = (directory / "report.pdf").read_bytes()
                assert pdf.startswith(b"%PDF-") and pdf.rstrip().endswith(b"%%EOF")

            path = directory / "Расчёт.xmcd"
            tool("export_xmcd", path=path.name)
            assert path.is_file()
            tool("set_input", index=2, value=48)
            tool("export_xmcd", path=path.name, error="Файл уже существует")
            tool("export_xmcd", path=path.name, overwrite=True)
            assert tool("open_sheet", path=path.name)["summary"]["errors"] == 0
            assert tool("get_region", index=6)["value"] == "1200 kPa"
            tool("set_input", index=2, value=80)
            external = path.read_bytes() + b"\n"
            path.write_bytes(external)
            tool("export_xmcd", path=path.name, overwrite=True, error="изменён на диске")
            assert path.read_bytes() == external
            tool("open_sheet", path=path.name, discard_unsaved=True)
            assert tool("get_region", index=6)["value"] == "1200 kPa"

            process.stdin.close()
            assert process.wait(timeout=10) == 0
            assert not process.stderr.read()
        finally:
            if process.poll() is None:
                process.kill()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--pdf", action="store_true", help="also test real browser PDF export")
    args = parser.parse_args()
    executable = args.executable.resolve(strict=True)
    with TemporaryDirectory(prefix="list-mcp-check-") as scratch:
        check(executable, Path(scratch), args.pdf)
    print("MCP stdio: 18 tools, 3 resources, calculations and file round trip passed.")
    if args.pdf:
        print("MCP PDF export passed.")
