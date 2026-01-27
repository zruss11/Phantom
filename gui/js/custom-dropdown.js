/**
 * Custom Dropdown Component
 * Provides a styled dropdown matching the slash-command aesthetic
 * Supports keyboard navigation, tooltips, dynamic option updates, and fuzzy search
 */
(function() {
  'use strict';

  /**
   * CustomDropdown constructor
   * @param {Object} options - Configuration options
   * @param {HTMLElement} options.container - The container element to render into
   * @param {Array} options.items - Array of {value, name, description} objects
   * @param {string} options.placeholder - Placeholder text when no selection
   * @param {string} options.defaultValue - Default selected value
   * @param {Function} options.onChange - Callback when selection changes
   * @param {boolean} options.searchable - Enable fuzzy search filtering
   * @param {string} options.searchPlaceholder - Placeholder text for search input
   */
  function CustomDropdown(options) {
    this.container = options.container;
    this.items = options.items || [];
    this.placeholder = options.placeholder || 'Select...';
    this.value = options.defaultValue || '';
    this.onChange = options.onChange || function() {};
    this.searchable = options.searchable || false;
    this.searchPlaceholder = options.searchPlaceholder || 'Search...';
    this.isOpen = false;
    this.selectedIndex = -1;
    this.focusedIndex = -1;
    this.trigger = null;
    this.panel = null;
    this.tooltip = null;
    this.tooltipTimeout = null;
    this.searchInput = null;
    this.itemsContainer = null;
    this.filteredItems = [];
    this.searchQuery = '';

    this.init();
  }

  CustomDropdown.prototype = {
    /**
     * Initialize the dropdown component
     */
    init: function() {
      this.createElements();
      this.attachEventListeners();
      this.updateTriggerText();
    },

    /**
     * Create the DOM elements
     */
    createElements: function() {
      // Clear container
      this.container.innerHTML = '';
      this.container.classList.add('custom-dropdown-wrapper');

      // Create trigger button
      this.trigger = document.createElement('button');
      this.trigger.type = 'button';
      this.trigger.className = 'custom-dropdown-trigger';
      this.trigger.innerHTML = '<span class="trigger-text"></span><span class="trigger-chevron">&#x25BC;</span>';
      this.container.appendChild(this.trigger);

      // Create dropdown panel
      this.panel = document.createElement('div');
      this.panel.className = 'custom-dropdown-panel';
      this.panel.tabIndex = -1;
      if (this.searchable) {
        this.panel.classList.add('searchable');
      }
      this.panel.style.display = 'none';
      this.container.appendChild(this.panel);

      // Create search input if searchable
      if (this.searchable) {
        var searchWrapper = document.createElement('div');
        searchWrapper.className = 'custom-dropdown-search-wrapper';

        this.searchInput = document.createElement('input');
        this.searchInput.type = 'text';
        this.searchInput.className = 'custom-dropdown-search';
        this.searchInput.placeholder = this.searchPlaceholder;
        this.searchInput.autocomplete = 'off';
        this.searchInput.spellcheck = false;

        searchWrapper.appendChild(this.searchInput);
        this.panel.appendChild(searchWrapper);
      }

      // Create items container
      this.itemsContainer = document.createElement('div');
      this.itemsContainer.className = 'custom-dropdown-items';
      this.panel.appendChild(this.itemsContainer);

      // Create tooltip (appears on left side when hovering options)
      this.tooltip = document.createElement('div');
      this.tooltip.className = 'custom-dropdown-tooltip';
      this.tooltip.style.display = 'none';
      this.container.appendChild(this.tooltip);

      // Initialize filtered items
      this.filteredItems = this.items.slice();

      // Render initial options
      this.renderOptions();
    },

    /**
     * Attach event listeners
     */
    attachEventListeners: function() {
      var self = this;

      // Toggle dropdown on trigger click
      this.trigger.addEventListener('click', function(e) {
        e.preventDefault();
        e.stopPropagation();
        self.toggle();
      });

      // Keyboard navigation on trigger
      this.trigger.addEventListener('keydown', function(e) {
        switch (e.key) {
          case 'Enter':
          case ' ':
          case 'ArrowDown':
            e.preventDefault();
            if (!self.isOpen) {
              self.open();
            }
            break;
          case 'Escape':
            if (self.isOpen) {
              e.preventDefault();
              self.close();
            }
            break;
        }
      });

      // Panel keyboard navigation
      this.panel.addEventListener('keydown', function(e) {
        self.onPanelKeydown(e);
      });

      // Search input listeners (if searchable)
      if (this.searchable && this.searchInput) {
        this.searchInput.addEventListener('input', function(e) {
          self.onSearchInput(e.target.value);
        });

        this.searchInput.addEventListener('keydown', function(e) {
          self.onSearchKeydown(e);
        });
      }

      // Click outside to close
      document.addEventListener('click', function(e) {
        if (!self.container.contains(e.target)) {
          self.close();
        }
      });

      // Mouse leave panel hides tooltip
      this.panel.addEventListener('mouseleave', function() {
        self.hideTooltip();
      });
    },

    /**
     * Handle keydown events on the panel
     */
    onPanelKeydown: function(e) {
      var self = this;
      var itemCount = this.searchable ? this.filteredItems.length : this.items.length;

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          this.focusedIndex = Math.min(this.focusedIndex + 1, itemCount - 1);
          this.updateFocus();
          break;

        case 'ArrowUp':
          e.preventDefault();
          this.focusedIndex = Math.max(this.focusedIndex - 1, 0);
          this.updateFocus();
          break;

        case 'Enter':
          e.preventDefault();
          if (this.focusedIndex >= 0 && this.focusedIndex < itemCount) {
            this.selectFilteredItem(this.focusedIndex);
          }
          break;

        case 'Tab':
          this.close();
          break;

        case 'Escape':
          e.preventDefault();
          this.close();
          this.trigger.focus();
          break;
      }
    },

    /**
     * Handle search input changes
     */
    onSearchInput: function(query) {
      this.searchQuery = query.toLowerCase().trim();
      this.filterItems();
      this.focusedIndex = this.filteredItems.length > 0 ? 0 : -1;
      this.renderOptions();
      this.updateFocus();
    },

    /**
     * Handle keydown on search input
     */
    onSearchKeydown: function(e) {
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          this.focusedIndex = Math.min(this.focusedIndex + 1, this.filteredItems.length - 1);
          this.updateFocus();
          break;

        case 'ArrowUp':
          e.preventDefault();
          this.focusedIndex = Math.max(this.focusedIndex - 1, 0);
          this.updateFocus();
          break;

        case 'Enter':
          e.preventDefault();
          if (this.focusedIndex >= 0 && this.focusedIndex < this.filteredItems.length) {
            this.selectFilteredItem(this.focusedIndex);
          }
          break;

        case 'Escape':
          e.preventDefault();
          this.close();
          this.trigger.focus();
          break;
      }
    },

    /**
     * Filter items based on search query (fuzzy matching)
     */
    filterItems: function() {
      var self = this;
      if (!this.searchQuery) {
        this.filteredItems = this.items.slice();
        return;
      }

      this.filteredItems = this.items.filter(function(item) {
        return self.fuzzyMatch(item.name, self.searchQuery) ||
               (item.description && self.fuzzyMatch(item.description, self.searchQuery));
      });

      // Sort by match quality (exact prefix match first, then contains, then fuzzy)
      this.filteredItems.sort(function(a, b) {
        var aName = a.name.toLowerCase();
        var bName = b.name.toLowerCase();
        var query = self.searchQuery;

        // Exact prefix match gets highest priority
        var aPrefix = aName.startsWith(query);
        var bPrefix = bName.startsWith(query);
        if (aPrefix && !bPrefix) return -1;
        if (!aPrefix && bPrefix) return 1;

        // Contains match gets second priority
        var aContains = aName.indexOf(query) !== -1;
        var bContains = bName.indexOf(query) !== -1;
        if (aContains && !bContains) return -1;
        if (!aContains && bContains) return 1;

        return 0;
      });
    },

    /**
     * Fuzzy match a string against a query
     */
    fuzzyMatch: function(str, query) {
      str = str.toLowerCase();
      var queryIndex = 0;
      for (var i = 0; i < str.length && queryIndex < query.length; i++) {
        if (str[i] === query[queryIndex]) {
          queryIndex++;
        }
      }
      return queryIndex === query.length;
    },

    /**
     * Return text for display (no highlighting to avoid spacing issues)
     */
    highlightMatch: function(text, query) {
      return document.createTextNode(text);
    },

    /**
     * Select item from filtered list
     */
    selectFilteredItem: function(filteredIndex) {
      if (filteredIndex < 0 || filteredIndex >= this.filteredItems.length) return;

      var item = this.filteredItems[filteredIndex];
      var originalIndex = this.items.indexOf(item);
      this.selectItem(originalIndex);
    },

    /**
     * Render the dropdown options
     */
    renderOptions: function() {
      var self = this;
      var container = this.itemsContainer || this.panel;

      // Only clear items container, preserve search input
      if (this.itemsContainer) {
        this.itemsContainer.innerHTML = '';
      } else {
        this.panel.innerHTML = '';
      }

      // Use filtered items for searchable dropdowns, all items otherwise
      var itemsToRender = this.searchable ? this.filteredItems : this.items;

      // Show no results message if search yields nothing
      if (this.searchable && this.searchQuery && itemsToRender.length === 0) {
        var noResults = document.createElement('div');
        noResults.className = 'custom-dropdown-no-results';
        noResults.textContent = 'No matching branches';
        container.appendChild(noResults);
        return;
      }

      itemsToRender.forEach(function(item, filteredIndex) {
        var originalIndex = self.items.indexOf(item);
        var div = document.createElement('div');
        div.className = 'custom-dropdown-item';
        if (item.value === self.value) {
          div.classList.add('selected');
          self.selectedIndex = originalIndex;
        }
        div.dataset.index = filteredIndex;
        div.dataset.originalIndex = originalIndex;
        div.tabIndex = -1;

        var nameSpan = document.createElement('span');
        nameSpan.className = 'item-name';

        // Highlight matching characters if searching
        if (self.searchable && self.searchQuery) {
          nameSpan.appendChild(self.highlightMatch(item.name, self.searchQuery));
        } else {
          nameSpan.textContent = item.name;
        }
        div.appendChild(nameSpan);

        if (item.description) {
          var descSpan = document.createElement('span');
          descSpan.className = 'item-description';
          descSpan.textContent = item.description;
          div.appendChild(descSpan);
        }

        // Click to select (use original index for non-searchable, filtered index for searchable)
        div.addEventListener('mousedown', function(e) {
          e.preventDefault();
          if (self.searchable) {
            self.selectFilteredItem(filteredIndex);
          } else {
            self.selectItem(originalIndex);
          }
        });

        // Hover to show tooltip
        div.addEventListener('mouseenter', function() {
          self.focusedIndex = filteredIndex;
          self.updateFocus();
          if (item.description) {
            self.showTooltip(item.description, div);
          }
        });

        div.addEventListener('mouseleave', function() {
          self.hideTooltip();
        });

        container.appendChild(div);
      });
    },

    /**
     * Update visual focus indicator
     */
    updateFocus: function() {
      var container = this.itemsContainer || this.panel;
      var items = container.querySelectorAll('.custom-dropdown-item');
      var self = this;
      var itemsList = this.searchable ? this.filteredItems : this.items;

      items.forEach(function(item, idx) {
        item.classList.toggle('focused', idx === self.focusedIndex);
      });

      // Scroll focused item into view (only within the panel, not ancestors)
      if (this.focusedIndex >= 0 && items[this.focusedIndex]) {
        var item = items[this.focusedIndex];
        var scrollContainer = this.itemsContainer || this.panel;
        var itemTop = item.offsetTop;
        var itemBottom = itemTop + item.offsetHeight;
        var scrollTop = scrollContainer.scrollTop;
        var containerHeight = scrollContainer.clientHeight;

        // Only scroll if item is outside visible area
        if (itemTop < scrollTop) {
          scrollContainer.scrollTop = itemTop;
        } else if (itemBottom > scrollTop + containerHeight) {
          scrollContainer.scrollTop = itemBottom - containerHeight;
        }
      }

      // Update tooltip for keyboard navigation
      if (this.focusedIndex >= 0 && this.focusedIndex < itemsList.length) {
        var focusedItem = itemsList[this.focusedIndex];
        var focusedEl = items[this.focusedIndex];
        if (focusedItem && focusedItem.description && focusedEl) {
          this.showTooltip(focusedItem.description, focusedEl);
        } else {
          this.hideTooltip();
        }
      }
    },

    /**
     * Show tooltip with description
     */
    showTooltip: function(description, itemEl) {
      var self = this;

      // Clear any pending hide timeout
      if (this.tooltipTimeout) {
        clearTimeout(this.tooltipTimeout);
        this.tooltipTimeout = null;
      }

      // Position tooltip relative to the panel for stable alignment
      var panelRect = this.panel.getBoundingClientRect();
      var containerRect = this.container.getBoundingClientRect();

      this.tooltip.textContent = description;
      this.tooltip.style.display = 'block';

      var tooltipRect = this.tooltip.getBoundingClientRect();
      var panelOffsetLeft = this.panel.offsetLeft;
      var panelOffsetTop = this.panel.offsetTop;
      var topOffset = panelOffsetTop + itemEl.offsetTop - this.panel.scrollTop;

      // Default: left of the panel
      this.tooltip.style.left = (panelOffsetLeft - tooltipRect.width - 8) + 'px';
      this.tooltip.style.right = 'auto';
      this.tooltip.style.top = topOffset + 'px';

      // If the tooltip would go off-screen on the left, flip to the right side
      var absoluteLeft = containerRect.left + panelOffsetLeft - tooltipRect.width - 8;
      if (absoluteLeft < 8) {
        this.tooltip.style.left = (panelOffsetLeft + panelRect.width + 8) + 'px';
      }
    },

    /**
     * Hide the tooltip
     */
    hideTooltip: function() {
      var self = this;
      // Small delay to prevent flicker when moving between items
      this.tooltipTimeout = setTimeout(function() {
        self.tooltip.style.display = 'none';
      }, 50);
    },

    /**
     * Select an item by index
     */
    selectItem: function(index) {
      if (index < 0 || index >= this.items.length) return;

      var item = this.items[index];
      var oldValue = this.value;
      this.value = item.value;
      this.selectedIndex = index;

      this.updateTriggerText();
      this.renderOptions();
      this.close();
      this.trigger.focus();

      if (oldValue !== this.value) {
        this.onChange(this.value, item);
      }
    },

    /**
     * Update the trigger button text
     */
    updateTriggerText: function() {
      var textEl = this.trigger.querySelector('.trigger-text');
      var selectedItem = this.items.find(function(item) {
        return item.value === this.value;
      }, this);

      if (selectedItem) {
        textEl.textContent = selectedItem.name;
      } else if (this.value && this.value !== 'default') {
        textEl.textContent = this.value;
      } else {
        textEl.textContent = this.placeholder;
      }
    },

    /**
     * Open the dropdown
     */
    open: function() {
      if (this.isOpen) return;
      this.isOpen = true;
      this.trigger.classList.add('open');
      this.panel.style.display = this.searchable ? 'flex' : 'block';

      // Reset search state for searchable dropdowns
      if (this.searchable) {
        this.searchQuery = '';
        this.filteredItems = this.items.slice();
        if (this.searchInput) {
          this.searchInput.value = '';
        }
        this.renderOptions();
      }

      // Set initial focus to selected item or first item
      var itemsList = this.searchable ? this.filteredItems : this.items;
      this.focusedIndex = this.selectedIndex >= 0 ? this.selectedIndex : 0;
      if (this.focusedIndex >= itemsList.length) {
        this.focusedIndex = 0;
      }
      this.updateFocus();

      // Position dropdown (below trigger)
      var self = this;
      if (!this.boundPositionPanel) {
        this.boundPositionPanel = function() {
          self.positionPanel();
        };
      }
      window.addEventListener('resize', this.boundPositionPanel);
      window.addEventListener('scroll', this.boundPositionPanel, true);

      // Position first, then focus after a frame to prevent scroll issues
      requestAnimationFrame(function() {
        self.positionPanel();

        // Focus search input or panel for keyboard navigation (after positioning)
        // Use preventScroll to avoid scrolling the page
        requestAnimationFrame(function() {
          if (self.searchable && self.searchInput) {
            self.searchInput.focus({ preventScroll: true });
          } else {
            self.panel.focus({ preventScroll: true });
          }
        });
      });
    },

    /**
     * Position the dropdown panel
     */
    positionPanel: function() {
      if (!this.panel || this.panel.style.display === 'none') return;
      var triggerRect = this.trigger.getBoundingClientRect();
      var safeInset = 16;
      var maxHeight = 280;
      var viewportHeight = window.innerHeight || document.documentElement.clientHeight;
      var viewportWidth = window.innerWidth || document.documentElement.clientWidth;
      var spaceBelow = Math.max(0, viewportHeight - safeInset - triggerRect.bottom - 6);
      var spaceAbove = Math.max(0, triggerRect.top - safeInset - 6);
      var desiredHeight = Math.min(this.panel.scrollHeight || 0, maxHeight);
      var openAbove = spaceBelow < desiredHeight && spaceAbove > spaceBelow;
      var allowedHeight = Math.min(maxHeight, openAbove ? spaceAbove : spaceBelow);

      var left = Math.min(
        Math.max(triggerRect.left, safeInset),
        Math.max(safeInset, viewportWidth - safeInset - triggerRect.width)
      );

      this.panel.style.position = 'fixed';
      this.panel.style.left = Math.round(left) + 'px';
      this.panel.style.right = 'auto';
      this.panel.style.width = Math.round(triggerRect.width) + 'px';
      this.panel.style.bottom = 'auto';
      this.panel.style.marginTop = '0';
      this.panel.style.marginBottom = '0';
      this.panel.style.maxHeight = Math.round(allowedHeight) + 'px';

      if (openAbove) {
        this.panel.classList.add('position-above');
        var panelHeight = Math.min(desiredHeight, allowedHeight);
        this.panel.style.top = Math.round(triggerRect.top - panelHeight - 6) + 'px';
      } else {
        this.panel.classList.remove('position-above');
        this.panel.style.top = Math.round(triggerRect.bottom + 6) + 'px';
      }
    },

    /**
     * Close the dropdown
     */
    close: function() {
      if (!this.isOpen) return;
      this.isOpen = false;
      this.trigger.classList.remove('open');
      this.panel.style.display = 'none';
      if (this.boundPositionPanel) {
        window.removeEventListener('resize', this.boundPositionPanel);
        window.removeEventListener('scroll', this.boundPositionPanel, true);
      }
      this.hideTooltip();
      this.focusedIndex = -1;

      // Reset search state
      if (this.searchable) {
        this.searchQuery = '';
        this.filteredItems = this.items.slice();
        if (this.searchInput) {
          this.searchInput.value = '';
        }
      }
    },

    /**
     * Toggle dropdown open/closed
     */
    toggle: function() {
      if (this.isOpen) {
        this.close();
      } else {
        this.open();
      }
    },

    /**
     * Set the dropdown options
     * @param {Array} items - Array of {value, name, description} objects
     */
    setOptions: function(items) {
      this.items = items || [];

      // Reset filtered items for searchable dropdowns
      if (this.searchable) {
        this.filteredItems = this.items.slice();
        this.searchQuery = '';
        if (this.searchInput) {
          this.searchInput.value = '';
        }
      }

      this.renderOptions();

      // Validate current selection still exists
      var currentExists = this.items.some(function(item) {
        return item.value === this.value;
      }, this);

      if (!currentExists && this.items.length > 0) {
        this.value = this.items[0].value;
        this.selectedIndex = 0;
      }

      this.updateTriggerText();
    },

    /**
     * Get the current value
     * @returns {string}
     */
    getValue: function() {
      return this.value;
    },

    /**
     * Set the current value
     * @param {string} value
     */
    setValue: function(value) {
      var index = this.items.findIndex(function(item) {
        return item.value === value;
      });

      if (index >= 0) {
        this.value = value;
        this.selectedIndex = index;
        this.updateTriggerText();
        this.renderOptions();
      } else if (value === 'default' || !value) {
        this.value = 'default';
        this.selectedIndex = 0;
        this.updateTriggerText();
        this.renderOptions();
      }
    },

    /**
     * Destroy the dropdown and clean up
     */
    destroy: function() {
      if (this.tooltipTimeout) {
        clearTimeout(this.tooltipTimeout);
      }
      this.container.innerHTML = '';
      this.container.classList.remove('custom-dropdown-wrapper');
    }
  };

  // Export to global scope
  window.CustomDropdown = CustomDropdown;
})();
