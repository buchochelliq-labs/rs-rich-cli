A paragraph with an *unclosed emphasis and **strong** text, `interpreted`,
``literal with *stars*``, a :kbd:`Ctrl+C`, :file:`{name}.txt`,
:menuselection:`File --> Open`, :command:`ls`, :pep:`8#section`, :rfc:`2616`,
:cve:`2024-1234`, :pypi:`rich`, :dfn:`term`, :sup:`x` and :sub:`y`.

Email someone@example.org or visit <https://example.com/x>. A footnote [#note]_
and again [#note]_. [*]_

.. [#note] Named auto footnote.
.. [*] Symbol.

Nested *emphasis with ``literal`` inside* and \*escaped\* and trailing\\.

A line with a trailing double colon::

  indented by two

.. _internal:

Paragraph targeted by an internal link, see internal_.

.. code:: python
   :name: example

   print(1)

.. admonition:: My Title

   Body with **bold**.

.. warning:: Short.

.. attention:: Long text in an attention that goes on and on to wrap at forty columns.
