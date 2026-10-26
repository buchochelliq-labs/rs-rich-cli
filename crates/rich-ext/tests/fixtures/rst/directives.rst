Directives
==========

.. versionadded:: 1.2
   The *spam* parameter.

.. versionchanged:: 2.0

.. deprecated:: 3.0
   Use ``other`` instead.

.. seealso:: The other module.

.. note::
   First paragraph.

   Second paragraph.

.. list-table:: Frozen delights
   :header-rows: 1

   * - Treat
     - Quantity
   * - Albatross
     - 2.99
   * - Crunchy Frog
     - 1.49

.. figure:: picture.png
   :alt: A figure

   The caption.

   The legend, which is
   a paragraph.

.. parsed-literal::

   some *emphasis* and **strong** text

.. epigraph::

   No matter where you go, there you are.

   -- Buckaroo Banzai

.. code-block:: rust
   :linenos:
   :emphasize-lines: 2

   fn main() {
       println!("hi");
   }

.. math::

   \alpha + \beta \leq \frac{a}{b}

Inline math :math:`\pi r^2` and :abbr:`LIFO (last-in, first-out)`.

.. |logo| image:: logo.png
.. |name| replace:: *Replaced*

A |logo| and |name|, an `anonymous link`__ and a `missing`_ one.

__ https://example.org

.. index:: ignored
