"""Fixed offline test peer. Not a production launcher or network helper."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'swarm-app'))
from seiya.reader_frame import parse_request, validate_response

request = parse_request(sys.stdin.buffer.read(16385))
response = Path(__file__).with_name('response.json').read_bytes()
validate_response(request, response)
sys.stdout.buffer.write(response)
