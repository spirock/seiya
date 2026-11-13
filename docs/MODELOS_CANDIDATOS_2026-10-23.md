# Candidatos DeepSeek/GLM · comprobación previa a instalación

2026-10-23. Evaluación previa de DeepSeek-V4.1-Flash-1017 y GLM Flash.
Se verificaron fuentes oficiales y recursos antes de descargar.
**No se descargaron pesos, instalaron paquetes ni modificaron servicios.**

## Identidad y tamaño observados

| Modelo | Revisión HF consultada | Archivos safetensors | Bytes de pesos |
|---|---|---:|---:|
| deepseek-ai/DeepSeek-V4.1-Flash-1017 | 1bae0e1088993b9b82de08f59de5be5a34e42170 | 48 | 510296708312 |
| zai-org/GLM-5.3-Flash | 46a050bec13002ad29ee795c16e61cf4fa3c4a9c | 62 | 328337455672 |

Tamaños sumados desde `siblings[].size` de la API HF `?blobs=true`, solo archivos
`.safetensors`; todos tenían tamaño. Son almacenamiento de los checkpoints
oficiales, no requisito exacto de RAM ni footprint medido. Licencia declarada MIT
en ambos; no se incorporó código. Revisión de documentación, no inferencia real.

Fuentes:

- https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017
- https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017/raw/main/README.md
- https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017/resolve/main/config.json
- https://huggingface.co/zai-org/GLM-5.3-Flash
- https://huggingface.co/zai-org/GLM-5.3-Flash/raw/main/README.md
- https://huggingface.co/zai-org/GLM-5.3-Flash/resolve/main/config.json

GLM oficial declara320B parámetros totales/18B activos; config glm5_next híbrida
lineal/sparse y MoE. Parámetros activos reducen cómputo, no equivalen a pesos
residentes únicos ni permiten descargar expertos sin mecanismo compatible.
DeepSeek declara arquitectura deepseek_v41, cuantización mixta FP8/experts FP4,
384 expertos en config. No se asume compatibilidad del MLX instalado.

DeepSeek indica que no entrega chat template Jinja y proporciona encoding propio
y deepseek-recipe; no tratarlo como sustitución directa de Gemma. GLM documenta
reasoning_effort low/high/max y clear_thinking; no trasladar automáticamente
enable_thinking=false del plan Gemma a estos modelos.

## Bloqueo local

`df -h "$HOME/colibri-models"`:58 GiB disponibles en volumen de460 GiB;
host documentado Apple M4/16 GiB. Ninguno de los checkpoints oficiales cabe en
el espacio libre, ni es candidato a carga residente completa en16 GiB. No se
borra otro modelo ni se descarga a medias para simular instalación.

Incluso la cuenta teórica de320B a4 bits es160 GB de valores antes de escalas,
buffers y contexto; no es promesa de formato soportado ni consumo real. No se
reactiva el experimento MoE/SSD aplazado para justificar estas descargas.

## Opciones pendientes de elección

1. Pruebas por API del modelo exacto: autorización específica de red/coste y
   credenciales suministradas por un canal seguro; fixtures sintéticas, sin datos
   personales. Es prueba remota, no instalación local ni baseline de memoria MLX.
2. Otro equipo con capacidad suficiente, ubicación y runtime autorizados.
3. Buscar un modelo/cuántización local más pequeño y revisar soporte/licencia/
   tamaño/calidad de tools antes de descargar. No sustituir estos modelos sin
   avisar: un modelo menor/distilado o versión GLM anterior no es el mismo checkpoint.

Seiya conserva identidad pública seiya/pegassus. La evaluación de candidatos no
cambia proveedor, instala un runtime o usa API remota como fallback oculto.
S3-05 y su calibración Gemma siguen pendientes de ejecución/autorización concreta;
esta investigación no constituye una baseline ni cancela el trabajo anterior.

## Selección posterior: DeepSeek-R1-Distill-Qwen-7B

La evaluación posterior selecciona la
conversión **mlx-community/DeepSeek-R1-Distill-Qwen-7B-4bit**, no el V4.1 ni GLM.
Base declarada: deepseek-ai/DeepSeek-R1-Distill-Qwen-7B. Modelo denso destilado,
no MoE. La conversión declara MLX-LM0.21.1 como herramienta de conversión; eso no
demuestra compatibilidad del runtime instalado sin prueba.

- Revisión fijada:21848dbf533d2518a1ef895104820d5ee51317ea.
- Pesos:4284346255 bytes (~4,28 GB decimales), tokenizer11422778 bytes.
- SHA-256 LFS de pesos:75ed9ed7ad66720041dd186dd0d8ce8a2dd8f487ea3f45a4b8fcaa7bbbf1f64f.
- SHA-256 LFS de tokenizer:e20ddafc659ba90242154b55275402edeca0715e5dbb30f56815a4ce081f4893.
- Config qwen2,4 bits/grupo64,28 capas,4 cabezas KV, hidden3584. Fuente qwen2.py
  presente en el MLX-LM existente, sin importarlo/cargar modelo.
- Destino nuevo: `~/colibri-models/mlx-deepseek-r1-distill-qwen-7b-4bit`.
  Descarga completada:7 archivos, tamaños comprobados y hashes SHA-256 registrados;
  pesos/tokenizer contrastados con hashes LFS publicados. Manifest local en
  `download-manifest.json`, completed=true, inference_tested=false. Revisión fija,
  sin instalar paquetes o ejecutar código remoto. No anunciar modelo probado por descarga.

Licencia de base declarada MIT; el README oficial señala origen Qwen2.5 bajo
Apache2.0. La tarjeta de conversión no añade un campo license explícito. Conservar
referencias/procedencia, no atribuirle licencia independiente inventada.

Fuentes:

- https://huggingface.co/mlx-community/DeepSeek-R1-Distill-Qwen-7B-4bit
- https://huggingface.co/deepseek-ai/DeepSeek-R1-Distill-Qwen-7B

### Límites para pruebas Seiya

El template inicia `<think>` y usa un formato propio de mensajes/tool-results.
No demuestra tool calling nativo fiable ni que enable_thinking=false lo desactive.
No usar un distill de razonamiento como reemplazo automático del worker de tools
ni enviar su razonamiento interno al usuario. Evaluar chat, límites/salida y tools
por separado. No ejecutar herramientas a partir de prosa reparada.

El contrato de provenance actual limita metadata a2 MiB/archivo: tokenizer.json
de11,4 MB supera ese límite. Es un bloqueo de esa ruta de campaña, no evidencia
de incompatibilidad MLX. Requiere revisar el límite específicamente para el
tokenizador, con cotas/tests, antes de usar el driver de campaña para este modelo.
Tamaño en disco no equivale a RAM: no se inicia inferencia sin preflight/autorización
de prueba aislada; se conserva Gemma y la identidad pública seiya/pegassus.

**Actualización posterior S3-06a/ADR0017:** bloqueo de tamaño resuelto: solo los
payloads tokenizer.json/tokenizer.model admiten32 MiB (resto2 MiB, total48 MiB).
Verificador nativo pasó sobre este checkpoint, con pesos/tokenizer SHA-256
iguales a los de la descarga. Registró Python3.11.9, MLX0.32.4, MLX-LM0.32.2 y
Transformers5.20.1 por METADATA, sin importarlos ni iniciar inferencia. Evidencia:
`seiya-s3-06a-r1-provenance-b96398f1/provenance.json` en temporal OpenCode aprobado.
Esto actualiza solo el bloqueo del verificador descrito arriba, no calidad/tools.

### Tests autorizados · 2026-10-23

Pruebas posteriores a la descarga. Resultado parcial, sin elevarlo a modelo
validado para Seiya:

| Comprobación | Evidencia |
|---|---|
| Integridad de los7 archivos | Tamaños/SHA-256 coinciden con manifest; pesos/tokenizer también con LFS fijado |
| Contenedor de pesos | Header82823 bytes,735 entradas, offsets cubren payload sin gaps/overlap; índice coincide |
| Tokenizador real | tokenizers0.23.3, marcador sintético y prompt completo roundtrip exacto;19 tokens en ese prompt |
| Plantilla real | Jinja2 3.1.6 sandbox; tools suministrados no cambian prompt; no incorpora el esquema read |
| Prefijo de generación | Termina en `<think>`; no asumir que enable_thinking=false lo desactive |
| Inferencia/calidad/RAM asignada | No ejecutadas: presión del host warning (sysctl valor2) |
| Tool calling por inferencia | No ejecutado/validado; no se ejecutó ninguna herramienta |

Script `local-tools-tests/seiya_r1_static_audit.py` importa solo tokenizers/Jinja2
además de stdlib, no MLX/Transformers ni tensors. Hash streaming1 MiB, header
acotado2 MiB; no materializa pesos en arrays. Cuatro regresiones sintéticas de
índice/offsets/cotas/symlinks añadidas a make test; ninguna usa modelo/red/host.
Auditoría nativa repetida sobre revisión fija, ~2,04 s; evidencia final en
`seiya-r1-static-final-a9a092f2/{audit/report.json,audit.log,host-preflight.json}`
bajo temporal OpenCode aprobado. Primer resultado en `seiya-r1-static-3d1c7414`.

Puertos conocidos8010/8012/8081 libres, no prueba de exclusividad GPU universal.
Presión de aviso persistió al cerrar las pruebas: no se inició runtime, no se
pidió purgar memoria o matar aplicaciones ajenas. Reintentar inferencia requiere
presión normal y preflight actual; la autorización de tests no elimina ese gate.
Que el template ignore esquemas es un límite de integración observado, no un
score negativo medido del modelo ni prueba de imposibilidad absoluta de tool use.
