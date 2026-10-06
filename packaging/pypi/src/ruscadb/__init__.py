"""RuscaDB — placeholder de reserva del nombre en PyPI.

El binding Python real (PyO3 + maturin) se implementa en la Fase F5 del
roadmap (`docs/RuscaDB-roadmap.md` §8). Este modulo solo reserva el nombre
``ruscadb`` y expone la version para verificacion de publicacion.
"""

from __future__ import annotations

__version__ = "0.1.0"
__all__ = ["__version__", "placeholder"]


def placeholder() -> str:
    """Devuelve un mensaje indicando que el binding aun no esta disponible.

    Returns:
        Mensaje informativo sobre el estado del paquete.
    """
    return "RuscaDB Python bindings arrive in Phase F5."
