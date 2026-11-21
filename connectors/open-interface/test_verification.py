"""Comprueba el límite entre actuar y verificar sin abrir aplicaciones ni modelos."""
import base64
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import desktop_goal


class VerificationTest(unittest.IsolatedAsyncioTestCase):
    def test_dictation_does_not_include_follow_up_instruction(self):
        self.assertEqual(desktop_goal.literal_note("Abre Writer y escribe esta nota: Buenos dias Bruno. Deja el documento visible."),"Buenos dias Bruno")
        self.assertEqual(desktop_goal.literal_note('Escribe: «Hola. Deja el documento visible.» y guarda.'),"Hola. Deja el documento visible.")
        self.assertIsNone(desktop_goal.literal_note("Redacta una carta de bienvenida"))

    async def test_extra_words_rejected_even_when_model_reports_success(self):
        client=AsyncMock()
        client.call_tool.return_value=SimpleNamespace(isError=False,content=[SimpleNamespace(type="image",data="YWJj")])
        with patch.object(desktop_goal,"ask",return_value={"cumplido":True,"evidencia":"Buenos dias Bruno. Deja el documento visible."}):
            with self.assertRaisesRegex(RuntimeError,"no coincide"):
                await desktop_goal.verify(client,"Abre Writer","model",1234,[],"Buenos dias Bruno")
    async def test_rejects_unconfirmed_and_malformed_verdicts_without_retry(self):
        image = base64.b64encode(b"fresh frame").decode()
        for verdict in ({"cumplido": False, "evidencia": "menú"},
                        {"cumplido": "true", "evidencia": "texto"},
                        {"cumplido": True, "evidencia": " "}, None):
            client = AsyncMock()
            client.call_tool.return_value = SimpleNamespace(isError=False, content=[
                SimpleNamespace(type="image", data=image)])
            with self.subTest(verdict=verdict), patch.object(desktop_goal, "ask", return_value=verdict):
                with self.assertRaises(RuntimeError):
                    await desktop_goal.verify(client, "escribe una nota", "model", 1234, [])
                client.call_tool.assert_awaited_once_with("observe", {})

    async def test_evidence_and_capture_come_from_read_only_turn(self):
        client = AsyncMock()
        image = base64.b64encode(b"new verification frame").decode()
        client.call_tool.return_value = SimpleNamespace(isError=False, content=[
            SimpleNamespace(type="image", data=image)])
        with tempfile.TemporaryDirectory() as tmp, patch.object(desktop_goal, "ROOT", Path(tmp)), \
             patch.object(desktop_goal, "ask", return_value={"cumplido": True, "evidencia": "Hola Ana"}) as ask:
            result = await desktop_goal.verify(client, "saluda a Ana", "model", 1234,
                                               [{"action": "type_text", "args": {"text": "not visual evidence"}}])
            messages = ask.call_args.args[2]
            self.assertEqual(messages[-1]["images"], [image])
            self.assertNotIn("not visual evidence", repr(messages))
            self.assertIn("Hola Ana", result["summary"])
            self.assertEqual((Path(tmp)/result["capture"]).read_bytes(), b"new verification frame")
            client.call_tool.assert_awaited_once_with("observe", {})

    async def test_exact_document_check_rejects_extra_text_missed_by_vision(self):
        client = AsyncMock()
        client.call_tool.side_effect = [
            SimpleNamespace(isError=False, content=[SimpleNamespace(type="image", data="YWJj")]),
            SimpleNamespace(isError=False, content=[SimpleNamespace(type="text", text="false")]),
        ]
        with patch.object(desktop_goal, "ask", return_value={"cumplido": True, "evidencia": "Buenos días Bruno."}):
            with self.assertRaisesRegex(RuntimeError, "El texto del documento no coincide"):
                await desktop_goal.verify(client, "Abre Writer", "model", 1234, [], "Buenos dias Bruno")
        client.call_tool.assert_awaited_with("text_matches", {"expected": "Buenos dias Bruno"})
