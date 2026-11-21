# Qwen-Image en WSL (evaluación local)

Este perfil es independiente del chat Ollama privado administrado por Seiya.
El generador usa CPU y cuatro hilos. Los pesos del generador y del codificador,
junto con el VAE, suman 9,34 GiB en disco. No arrancar varias generaciones.

Ejemplo experimental validado en WSL2 en una prueba de chat con un perfil limpio. Copiar
`qwen_endpoint.py` y `catalog-qwen.json` a `~/.local/share/seiya-image/`. El
adaptador espera `runtime/build/bin/sd-cli` y los tres pesos bajo
`qwen-image-2.1-turbo-q4/` en ese directorio. Usar las revisiones, nombres
locales y hashes del [manifiesto](../../contracts/benchmarks/image-candidates/v0.1.0/qwen-image21-turbo-q4.json).

### Acceso único en WSL

Con los pesos, `sd-cli`, el catálogo y el adaptador ya preparados, instalar
`seiya-imagenes` en `~/.local/bin` y ejecutarlo. Inicia los dos motores y el
host sin enviar mensajes. Una segunda ejecución reabre la página del mismo
host en el navegador que conserva su cookie (requiere la versión con
reapertura Linux). Si otro perfil ya usa 8015, el lanzador no lo reemplaza.
Para detener el conjunto, pulsa Ctrl+C en su terminal.

### Alternativa manual

1. En una terminal, ejecutar el adaptador:

   ```sh
   python3 ~/.local/share/seiya-image/qwen_endpoint.py
   ```

2. En otra terminal, iniciar un motor dedicado de texto con el Qwen ya instalado:

   ```sh
   OLLAMA_HOST=127.0.0.1:21011 OLLAMA_NO_CLOUD=1 OLLAMA_NOPRUNE=1 \
     OLLAMA_MODELS="$HOME/.local/state/Seiya/ollama-models" ollama serve
   ```

3. Una vez detenida la instancia anterior de Seiya (puerto 8015), abrir el perfil:

   ```sh
   seiya-rust --launch --catalog "$HOME/.local/share/seiya-image/catalog-qwen.json"
   ```

   Revisar que el Cerebro sea `brain-tools`, el pipeline `image-local` y los
   campos de ejecutable/assets de imagen estén vacíos. El catálogo no reemplaza
   `local-models.json`; confirmar el perfil sí guarda las asignaciones de roles.
   El chat debe producir una llamada `image_generate`, no solo texto. La
   compatibilidad con tools depende del modelo de texto y de su plantilla.

Para cambiar de generador, registrar en `image_models` otro ID, binding
exacto y URL de su propio motor compatible. Seleccionarlo en `image` o desde
el selector de pipeline. Los parámetros del CLI de este adaptador son
específicos de Qwen-Image 2.1 Turbo; no apuntarlo a pesos de otra arquitectura.
Detener los dos motores externos con Ctrl+C en sus respectivas terminales.

### Prueba manual desde el chat

`test_chat_browser.py` usa Playwright para Python y Chrome. Con ambos motores
preparados, usar una instancia de evaluación con un perfil nuevo y guardar su
enlace inicial de un solo uso en un archivo privado, antes de abrirlo. Ejecutar:

```sh
python3 test_chat_browser.py /ruta/privada/enlace /ruta/nueva/bosque-chat.png
```

La prueba envía una petición real desde el chat, espera el trabajo asociado al
mensaje y descarga su PNG de 512×512. No usa el panel manual como alternativa ni
reenvía la petición si el motor falla.

En la prueba del 20 de noviembre de 2026, `qwen2.5:1.5b` propuso
`image_generate` para un bosque con niebla. Playwright descargó desde el
mensaje del asistente un PNG RGBA de 512×512 (530037 bytes). Su SHA-256,
`66106d6704a8e8b59ab6f94c2147c3e8d2863fddc5b7601bda4249aa9673c1ae`,
coincidió con la generación directa con el mismo prompt y semilla. Esta
prueba usa el generador en CPU y turnos secuenciales.
