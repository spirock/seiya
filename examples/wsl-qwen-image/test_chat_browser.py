"""Prueba manual con Playwright: usa una sesión de evaluación ya abierta y dos motores preparados."""
import argparse
import re
from pathlib import Path
from urllib.parse import urlsplit
from uuid import UUID


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("session_file", type=Path, help="Archivo privado con el enlace inicial de la instancia de prueba")
    parser.add_argument("output", type=Path, help="Nuevo archivo PNG")
    args = parser.parse_args()
    url = args.session_file.read_text().strip()
    parts = urlsplit(url)
    if (parts.scheme != "http" or parts.netloc != "127.0.0.1:8015" or parts.query or parts.fragment
            or not re.fullmatch(r"/session/[0-9a-f]{64}", parts.path)):
        parser.error("Se requiere el enlace inicial de la instancia de evaluación en loopback.")
    if args.output.exists():
        parser.error("La salida ya existe.")

    from playwright.sync_api import sync_playwright
    with sync_playwright() as pw:
        browser = pw.chromium.launch(channel="chrome")
        page = browser.new_page(accept_downloads=True)
        page.goto(url)
        page.get_by_role("button", name="Configurar mi modelo", exact=True).click()
        page.wait_for_function("document.querySelector('#setupBrain').value === 'brain-tools'")
        if page.locator("#setupImage").input_value() != "image-local":
            raise RuntimeError("El perfil no corresponde al catálogo de evaluación de Qwen-Image.")
        page.locator("#setupReview").click()
        page.locator("#setupStart").click()
        page.wait_for_function("""() => !document.querySelector('#setupEnter').hidden ||
            /Bloqueado/.test(document.querySelector('#setupEngineState').textContent)""")
        if not page.locator("#setupEnter").is_visible():
            raise RuntimeError("Arranque bloqueado: " + page.locator("#setupMessage").inner_text())
        page.locator("#setupEnter").click()
        page.locator("#prompt").fill("Genera una imagen de un bosque con niebla, árboles de pino y luz suave de la mañana.")
        with page.expect_response(lambda r: r.url.endswith("/api/chat"), timeout=150_000) as turn:
            page.locator("#send").click()
        reply = turn.value.json()
        if not turn.value.ok or not reply.get("image_job"):
            raise RuntimeError(f"No se registró un trabajo de imagen: {str(reply)[:700]}")
        # El acuse del chat no demuestra que exista un PNG.
        job_id = str(UUID(reply["image_job"]["job_id"]))
        card = page.locator(f'#history .message.assistant [data-seiya-image-progress] [data-image-job-id="{job_id}"]')
        download = card.get_by_role("button", name="Descargar PNG", exact=True)
        failed = card.locator("p").filter(has_text=re.compile(r"Falló|Bloqueada|Finalización incierta|^Cancelada$"))
        download.or_(failed).first.wait_for(state="visible", timeout=750_000)
        if failed.is_visible():
            raise RuntimeError(f"Trabajo {job_id}: {failed.inner_text()}")
        with page.expect_download() as result:
            download.click()
        artifact = result.value
        if artifact.failure():
            raise RuntimeError("Falló la descarga del PNG.")
        with artifact.path().open("rb") as source:
            png = source.read(16 * 1024 * 1024 + 1)
        if len(png) > 16 * 1024 * 1024 or png[:24] != bytes.fromhex("89504e470d0a1a0a0000000d494844520000020000000200"):
            raise RuntimeError("No se obtuvo el PNG 512×512 esperado.")
        with args.output.open("xb") as dest:
            dest.write(png)
        print(f"PNG del chat: {args.output} ({len(png)} bytes)")


if __name__ == "__main__":
    main()
