# Qwen-Image-2.1 · revisión previa a descarga

**Actualización posterior W2–W5:** seleccionada explícitamente variante Unsloth
Turbo Q4, tres assets10.023.773.264bytes descargados y verificados, runtime
privado sd.cpp/Metal compilado y una imagen512×512 generada/validada en167,335s
con supervisor Rust/macOS. Hubo presión crítica y swap alto; no admitir el par
LLM por este ensayo. Sin API de imágenes web. Estado/evidencia y límites en
[hoja de ruta pública](../ROADMAP.md) y
[operación](IMAGEN_LOCAL_EXPERIMENTAL.md). Las secciones siguientes conservan la
revisión histórica **anterior** a esa selección y pruebas; no describen el estado
actual de la variante elegida.

2026-11-11. La evaluación previa al broker de trabajos solo
leyó fuentes públicas/metadatos y espacio de disco: **sin descargar pesos,
instalar dependencias, cargar modelos o generar imágenes**.

## Referencia y tamaño

Repositorio oficial [Qwen/Qwen-Image-2.1](https://huggingface.co/Qwen/Qwen-Image-2.1),
revisión declarada por API `d26bb61231c349cf6b7896fa83353113880e1ba3`.
Pipeline QwenImage21Pipeline (Diffusers), no chat de Ollama. Inventario actual
de archivos safetensors principales:

| Componente | Bytes declarados por API |
|---|---:|
| Text encoder (4 shards) | 17.534.339.488 |
| Transformer visual (2 shards) | 14.230.284.408 |
| VAE | 1.350.989.512 |
| Total pesos, sin metadata/auxiliares | 33.115.613.408 |

Son tamaños de archivos, no memoria residente/peak. Los «7B» anunciados se
refieren al componente visual, no al pipeline completo con encoder/VAE.
Consulta local df sobre el volumen de modelos:≈14GiB libres. Insuficiente incluso
para esos pesos sin margen; no se inició transferencia o limpieza. Host16GiB,
sin runtime de imagen compatible/consumo validado. No atribuir soporte MPS/MLX
por tener un ejemplo de CUDA o un pipeline instalado.

## Aclaración sobre9B

Existen modelos oficiales **Qwen-Image-2.1-PE-T2I** y **PE-I2I**, basados en
Qwen3.5-VL9B. Sus tarjetas los describen como *prompt rewriting*: producen
instrucciones para el generador posterior, **no imágenes por sí solos**.
No descargarlos como sustituto ligero del generador. Si el usuario se refiere
a otra conversión/variante9B, hace falta su referencia exacta para verificarla.

El usuario aporta el repositorio oficial y se lee su README completo. La sección
Apple Silicon enlaza [LightX2V-Mac-App](https://github.com/ModelTC/LightX2V-Mac-App)
para **Qwen-Image-2.1-Turbo** local; no es el mismo paquete BF16 inventariado
arriba ni una integración de Seiya ya probada. Ruta de compatibilidad a revisar,
no permiso de instalar una app/paquetes, descargar otra variante o llamar su API.
No se afirma que no exista soporte Apple Silicon: falta validar esta máquina,
paquete, espacio y consumo para la variante que se elija explícitamente.

## Licencia y decisiones pendientes

La [licencia oficial](https://huggingface.co/Qwen/Qwen-Image-2.1/raw/main/LICENSE)
es Qwen Research License Agreement: uso no comercial para investigación/evaluación;
uso comercial requiere licencia aparte. No presentarla como Apache/MIT ni
distribuir pesos dentro del instalador de Seiya. Esto no sustituye revisión legal.

Bloqueos: espacio para pesos/margen, elección de variante/runtime compatible y
uso previsto/licencia. Autorizar descarga original no autoriza otra conversión,
instalar paquetes, usar API remota, borrar modelos o forzar memoria. No se cambia
de modelo/proveedor para esconderlos. Integrar jobs de imágenes requiere además
contrato de artefactos binarios, límites/cancelación/permisos propios; el broker
actual solo produce borradores de texto.

Fuentes leídas:

- [API oficial de inventario](https://huggingface.co/api/models/Qwen/Qwen-Image-2.1?blobs=true)
- [Repositorio oficial](https://github.com/QwenLM/Qwen-Image-2.1)
- [PE-T2I9B](https://huggingface.co/Qwen/Qwen-Image-2.1-PE-T2I)
- [PE-I2I9B](https://huggingface.co/Qwen/Qwen-Image-2.1-PE-I2I)

## Revisión posterior · web completa solicitada

Usuario quiere Gemma+auxiliares+imágenes en la misma web, no chat Qwen solo. Se
revisaron fuentes públicas/metadatos, sin descargar pesos, instalar o generar.
df fresco:15.728.368KiB≈15GiB libres en el volumen; host16GiB, Ollama vacío.

### Turbo oficial no resuelve el disco

`Qwen/Qwen-Image-2.1-Turbo`, revisión
fc13ba1986fcb905854dc8de2f52eee395f49b5b, pesos principales32.440.133.648bytes:
encoder17.534.340.584, transformer14.230.284.408, VAE675.508.656. Turbo reduce a
8 pasos, no convierte por sí solo el paquete completo en uno pequeño. Mismo
Qwen Research License (investigación/evaluación no comercial; comercial aparte).

LightX2V-Mac-App README dice probado en **M4 Pro24GB**, no16GB. Config MPS declara
cpu_offload por bloques, dit/text_encoder_disk_streaming, VAE tiling y resolución
1024; no evidencia local. Referencia consultada e07953d687d8357de0d0e6288cd18e615eace942.
API GitHub licencia=null y LICENSE raíz404 en esta revisión: no copiar/distribuir
su código por asumir licencia del motor. Motor LightX2V separado declara Apache2.0.
Nada instalado, ni pasos de Gatekeeper ejecutados.

### Candidato cuantizado, pendiente de elección explícita

Tarjeta Unsloth declara conversión de Turbo, denoiser solamente, y recomienda
estos3 componentes para stable-diffusion.cpp. Metadatos de LFS consultados:

| Repositorio/revisión | Archivo | Bytes | SHA256 declarado |
|---|---|---:|---|
| unsloth/Qwen-Image-2.1-Turbo-GGUF @ bd53e520a64472adaff780da0b2f2d7eb1605e30 | qwen-image-2.1-turbo-Q4_K_M.gguf |4199565120|8940e8b522af886e0c0f5afddf720e82dd223cb9a43580b5f61c4d740452ac20|
| unsloth/Qwen3-VL-8B-Instruct-GGUF @ b93a7ee713758252c555be4210c00540df954dc2 | Qwen3-VL-8B-Instruct-UD-Q4_K_XL.gguf |5148699488|e3d1a6e87c5cb31e054f2c3bc0dd82ffde052f613de5eef3665b7bd33c9b703e|
| unsloth/Qwen-Image-2.1-FP8 @ bb21a8ba7f0371c19ad1f72c82a5e8fc10f39939 | vae/qwen_image_2.1_vae_bf16.safetensors |675508656|71879ffd5321e6d10c3c87513e2b474b1252efa7f3dec2969214a9bf06a6dd5c|

Total10.023.773.264bytes≈9,34GiB, sin binario/build/metadata/swap/salidas. El
encoder es parte del pipeline, no un auxiliar PE9B; no intercambiable por el
chat Qwen3.5 solo por compartir familia. Repositorio FP8 se usa solo para ese
VAE BF16 exacto, no descargar todo el snapshot o cambiar dtype silenciosamente.
Licencias declaradas: denoiser/VAE Qwen Research; encoder Apache2.0, sin que eso
relicencie el pipeline. Son hashes de metadatos, **no descarga verificada**.

Runtime candidato: stable-diffusion.cpp, MIT, revisión
32d516c0197ed5ca31305b8575e14eb1451fad33. Docs propias QwenImage2.1 describen
Qwen3-VL8B+VAE específico, GGUF y preset Turbo8 pasos/CFG1/Euler. Build declara
Metal, pero advierte de ineficiencia en matrices grandes: no prometer rendimiento
o16GiB validados. mmproj adicional requerido para edición, no incluido en este
candidato inicial de texto→imagen. No instalar ComfyUI/LightX2V como fallback.

Cabe nominalmente en el disco observado dejando≈5,66GiB antes de build/swap,
no es garantía de headroom. Repetir df y reservar margen antes de toda descarga
o inferencia; no borrar pesos/backups para hacerlo caber. Ejecutar imagen aparte
de los LLM en una primera prueba aislada, no cargar los tres por defecto.

Fuentes adicionales, solo lectura:
- [Turbo oficial](https://huggingface.co/Qwen/Qwen-Image-2.1-Turbo)
- [Conversión propuesta](https://huggingface.co/unsloth/Qwen-Image-2.1-Turbo-GGUF)
- [Encoder propuesto](https://huggingface.co/unsloth/Qwen3-VL-8B-Instruct-GGUF)
- [VAE propuesto](https://huggingface.co/unsloth/Qwen-Image-2.1-FP8)
- [Runtime Qwen2.1](https://github.com/leejet/stable-diffusion.cpp/blob/32d516c0197ed5ca31305b8575e14eb1451fad33/docs/qwen_image_2.1.md)
- [Build Metal](https://github.com/leejet/stable-diffusion.cpp/blob/32d516c0197ed5ca31305b8575e14eb1451fad33/docs/build.md)
