# Seiya Rust · Windows x64 experimental

Entrega inicial: dos ejecutables Rust de consola, ZIP portable y setup.exe sin
firma. **No es todavía la aplicación web completa ni tiene inferencia/modelos
Windows validados.** No hay MLX/Metal, imágenes nativas, host --serve macOS ni
paridad desktop por compilar un .exe. No incluye Python, pesos o motores externos.

Descargas actuales: [bloque actualizado automáticamente en README](https://github.com/spirock/seiya#descargas).
Build confirmado: [Actions43507170152](https://github.com/spirock/seiya/actions/runs/43507170152),
commit1d49370. ZIP portable y setup.exe se verifican en ese run.
GitHub entrega cada artefacto envuelto en ZIP: descomprimir antes de usarlo.
Windows/MSVC, auditoría DLL y ayuda/config del ZIP pasaron; instalación no probada.

## Qué abrir

### Fuentes nuevas · catálogo HTTP local

El backend Rust de texto ya permite registrar y sustituir cerebro/trabajadores
mediante motores externos con API OpenAI local, por ejemplo llama.cpp, sin usar
MLX ni iniciar/descargar motores desde Seiya. Preparar `%LOCALAPPDATA%\Seiya\local-models.json`
según el ejemplo incluido y [la operación portable](BACKENDS_LOCALES.md).
`seiya-desktop.exe --check-catalog FILE` es offline; `--launch` abre el asistente.
Con catálogo, las asignaciones se guardan en documento propio con CAS y pueden
iniciar el chat bajo confirmación; sin catálogo no se inventa un modelo disponible.

Todavía no publicado ni compilado/probado de nuevo en Windows real en el entorno
local. El código/contrato se prueba con fixtures en macOS, no acredita DirectML,
CUDA, GPU/VRAM, todos los modelos, instalación o sesión larga en Windows. Se
conservan los assets, incluidas etiquetas heredadas macOS que no describen este
adaptador; seguir la advertencia HTTP y mantener vacías las rutas Ollama/MLX.
El bloque siguiente describe el incremento previo, no esta ampliación de backend.

**Fuentes actuales, aún no publicadas en el build anterior:** el acceso principal
de Inicio y `Seiya-Rust.cmd` invocan `seiya-desktop.exe --launch` y abren el mismo
asistente web que macOS, sin configuración previa. El gestor permite editar y
guardar IDs de roles solo como borrador en el navegador. No hay adaptador de
inferencia/selector nativo Windows ni perfil de motores Windows: la web lo indica
y bloquea el arranque. No descarga modelos ni usa un proveedor remoto.
El empaquetado se comprueba sin instalar o abrir el navegador en Windows.
Las instrucciones siguientes corresponden al build descargable anterior.

- ZIP: extraer todo a una carpeta propia. `Seiya-Rust-Help.cmd` muestra ayuda y
  mantiene la consola abierta. `seiya-desktop.exe --help`/`--version` funcionan;
  doble clic directo en una consola puede cerrarla al terminar.
- Setup: instalador per-user bajo `%LOCALAPPDATA%\Programs\Seiya Rust Experimental`,
  con acceso de ayuda en Inicio. No requiere admin. No instala servicios ni inicia
  servidores/modelos, no altera PATH/firewall ni descarga redistribuibles. Usa solo
  la entrada estándar de desinstalación por usuario de InnoSetup, sin claves extra.
- `seiya-server.exe`: API experimental existente, solo127.0.0.1. Ejemplo incluido
  **discovery sin runtime**; no intenta generar una respuesta con un modelo falso.

Desde PowerShell en la carpeta extraída/instalada:

```powershell
.\seiya-desktop.exe --help
.\seiya-desktop.exe --version
.\seiya-server.exe check-config --config .\server.discovery.example.json
# Solo si quieres abrir manualmente la API discovery local (sin modelos):
.\seiya-server.exe serve --config .\server.discovery.example.json
# Ctrl+C para detener únicamente ese proceso.
```

La configuración incluida no usa claves ni runtime. Para chat no basta con
añadir un modelo Windows a ciegas: el adaptador HTTP existente exige su contrato
específico y no hay backend nativo Windows validado en este incremento. El perfil
desktop JSON v1 y `seiya-desktop --serve` pertenecen a macOS; **no usarlos como
configuración Windows**. No cambiar proveedor/fallback o descargar pesos para
ocultar esa limitación. La API no expone UI de chat en raíz.
Guardar una configuración propia fuera de la carpeta instalada; el ejemplo
distribuido pertenece al paquete y puede sustituirse/retirarse al actualizar.

## Requisitos, verificación y seguridad

Target x86_64-pc-windows-msvc, Windows10(1809)+/11 x64 como destino de build;
compilación/comprobaciones se ejecutan en runner Windows Server2025, no acreditan
todos los PCs, versiones de Windows, ARM64/x86 o una sesión larga. Build CI pide
CRT estático y audita DLLs; no instala VC++ Redistributable. Dependencias del SO
siguen existiendo; Rust no significa binario sin dependencias o aceleración GPU.

`BUILD-INFO.json` contiene revisión explícita y SHA256 de ambos binarios;
`SHA256SUMS` acompaña ZIP/setup. Las cabeceras PE y hashes se verifican en staging
y ZIP extraído. Formato correcto no certifica procedencia: usar artefactos del run
del repositorio autorizado y comparar hashes. Sin Authenticode/reputación
SmartScreen; no se recomienda desactivar Defender/SmartScreen ni dar excepciones
globales. Instalación es decisión del propietario y no se prueba automáticamente.

El setup se compila con InnoSetup ya incluido en el runner, desde allowlist fija,
sin [Run]/[Code]/[Registry] personalizados ni autoarranque. El ZIP sí se extrae
y se ejecutan solo ayuda/versiones/check-config en CI. **Compilar el setup no
equivale a instalarlo, extraerlo ni verificar su desinstalación en un PC real.**
Python/InnoSetup son herramientas de build, no runtimes del payload. No se instala
ninguna de ellas en el equipo del usuario por ejecutar los binarios Rust.

Rollback: usar ZIP sin instalar o retirar el programa experimental desde Aplicaciones.
No borra modelos, conversaciones, locks ni configuración externa. No reemplaza
instalaciones Python/macOS/Linux, ni modifica OpenCode o servicios existentes.

## Build reproducible en alcance (sin instalar)

En runner Windows con herramientas ya presentes:

```powershell
$env:RUSTFLAGS='-C target-feature=+crt-static'
cargo build --release --locked --target x86_64-pc-windows-msvc -p seiya-server --bin seiya-desktop --bin seiya-server
python swarm-app/packaging/windows.py zip --bin-dir target/x86_64-pc-windows-msvc/release --output dist/windows-portable
```

Extraer ZIP e inspeccionar con `windows.py inspect --payload-dir CARPETA`;
`windows.py setup --payload-dir CARPETA --output dist/windows-setup --compiler
RUTA_ISCC` compila exactamente ese payload ya inspeccionado. `--revision SHA`
registra revisión solo al construir ZIP/staging desde binarios. Si falta ISCC,
se bloquea setup sin instalar herramientas de forma silenciosa.
