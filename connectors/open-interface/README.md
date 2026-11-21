# Conector opcional Open Interface · Linux/WSL

Desde esta carpeta, ejecutar `./install.sh` como el usuario que abre Seiya.
Requiere `uv`, Git, Xvfb, `xauth` y `xcalc`. Instala un entorno Python y el
intérprete de [Open Interface](https://github.com/AmberSahdev/Open-Interface)
en `~/.local/share/seiya-open-interface/`, conservando su licencia GPL-3.0.
No se incluyen en el instalador de Seiya ni se descarga ningún modelo con
este script.

**Primera operación:** `calculator.v1`. En el chat, escribir `/sumar 27 18`
o `abre la calculadora y suma 27 y 18`. Requiere un Cerebro o trabajador
asignado con visión confirmada por su motor Ollama local. El botón Enviar
invoca esta operación concreta; no es todavía una decisión autónoma del LLM.
Open Interface pulsa las teclas en una calculadora real abierta en Xvfb.
Solo se envía la imagen del visor a ese modelo en loopback. Si la lectura
coincide con la suma, el chat guarda el resultado observado como respuesta.
La pantalla personal no se captura. La calculadora de evaluación no se ve
en el escritorio del usuario.

### Escritorio MCP experimental

En el perfil asistido de Linux, escribe un objetivo y pulsa **Usar escritorio**.
El cliente entrega el objetivo y las capturas al Cerebro o trabajador visual
asignado. Open Interface aplica las acciones MCP elegidas por el modelo:
observar, abrir una aplicación del menú, pulsar, escribir o navegar con el teclado.
El menú inicial ofrece Calculadora y Writer. No hay una herramienta para crear
cartas ni una secuencia de clics de Writer.

La pantalla Xvfb y el perfil de aplicaciones son temporales; no es el escritorio
personal ni un aislamiento del sistema de archivos. Una captura final queda bajo
`~/.local/share/seiya-open-interface/` si el modelo comunica que terminó.
Una tarea repetitiva o de más de doce pasos se interrumpe. El resumen del modelo
no sustituye la verificación del resultado. El 21 de noviembre de 2026, tras
comprobar la clase y el foco de Writer antes de escribir y ejecutar la prueba
con el usuario normal de WSL, se escribieron tres notas desde el chat. Dos
tenían el contenido solicitado; en la tercera, el modelo incluyó una instrucción
de la petición («Deja el documento visible») y el verificador la dio por válida.
En la repetición se separó el dictado de la instrucción y se comprobó el texto
real de Writer antes de confirmar. Para pedir texto exacto, usa comillas:
`Abre Writer y escribe: «Buenos días, Bruno». Deja el documento visible.`
La confirmación se basa en la captura y en el contenido de Writer, no en la
transcripción aproximada de Gemma. El conector aún no se ejecuta en macOS.

Para desactivar el conector, retirar el ejecutable `seiya-open-interface`
de esa carpeta. El chat de texto no requiere el conector.

### Perfil verificado en WSL2

Con `qwen2.5:1.5b` en el almacén privado de Seiya y `gemma4:e2b` en
`~/.local/share/seiya-open-interface/model-store`, detener el perfil anterior
y ejecutar `~/.local/bin/seiya-asistido`. Seleccionar `brain-small` como Cerebro
y `visual-worker` como trabajador (sin imágenes) al revisar la configuración.
El lanzador no descarga modelos. La lectura visual de este primer conector usa
CPU y cuatro hilos para evitar saturar la VRAM de equipos pequeños. Permite
hasta tres minutos para la consulta local sin reintentar la operación.

En la prueba real con Playwright, el motor visual devolvió `45` para `/sumar 27 18`;
la respuesta quedó visible en el mismo chat tras recargar la página.
