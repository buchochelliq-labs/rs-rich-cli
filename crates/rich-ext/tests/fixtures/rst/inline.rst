Inline markup: *emphasis*, **strong**, ``literal``, `title ref`, :sub:`2`, H\ :sub:`2`\ O, E = mc\ :sup:`2`.
Escapes: \*not emphasis\*, 2 * 3 * 4, a*b*c, "*quoted*" and '*'.
Roles: :code:`x = 1`, :emphasis:`emph`, :strong:`bold`, :literal:`lit`, :func:`os.path.join`, :class:`Title <pkg.Class>`, :math:`a^2`, :bogus:`thing`, :PEP:`8`, :RFC:`2822`.
References: Python_, `Rust lang`_, `inline <https://rust-lang.org>`_, anonymous__, https://example.com/path?q=1., mail me@example.com, see [1]_ and [#]_ and [CIT2002]_.
Substitution |name| and |ref|_ here. Target _`inline target` here.

.. _Python: https://www.python.org
.. _Rust lang: https://www.rust-lang.org
__ https://anon.example.com
.. |name| replace:: replaced text
.. [1] First footnote.
.. [#] Auto footnote.
.. [CIT2002] A citation.
