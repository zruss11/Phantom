/**
 * Slash Command Autocomplete Component
 * Provides fuzzy-searchable command autocomplete for agent inputs
 * Supports both textarea and contenteditable elements
 */
(function() {
  'use strict';

  /**
   * SlashCommandAutocomplete constructor
   * @param {HTMLElement} element - The input element to attach to (textarea or contenteditable)
   * @param {string} agentId - The initial agent ID for command filtering
   */
  function SlashCommandAutocomplete(element, agentId) {
    this.element = element;
    this.agentId = agentId || 'codex';
    this.dropdown = null;
    this.selectedIndex = 0;
    this.filteredCommands = [];
    this.isVisible = false;
    this.triggerPosition = null;
    this.suppressBlur = false; // Flag to prevent blur from closing dropdown during agent switch
    this.isContentEditable = element.isContentEditable || element.contentEditable === 'true';

    this.init();
  }

  SlashCommandAutocomplete.prototype = {
    /**
     * Initialize the autocomplete component
     */
    init: function() {
      this.createDropdown();
      this.attachEventListeners();
      this.loadCommands();
    },

    /**
     * Create the dropdown element
     */
    createDropdown: function() {
      this.dropdown = document.createElement('div');
      this.dropdown.className = 'slash-command-dropdown';
      this.dropdown.style.display = 'none';

      // Insert dropdown near the element
      var parent = this.element.parentElement;
      if (parent) {
        parent.style.position = 'relative';
        parent.insertBefore(this.dropdown, this.element);
      }
    },

    /**
     * Calculate optimal dropdown position and size based on available space
     */
    positionDropdown: function() {
      if (!this.dropdown || !this.element) return;

      var textareaRect = this.element.getBoundingClientRect();
      var viewportHeight = window.innerHeight;
      var safeMargin = 16; // Keep dropdown 16px away from viewport edges
      var itemHeight = 44; // Approximate height of each command item
      var minVisibleItems = 3;
      var minDropdownHeight = itemHeight * minVisibleItems;

      // Calculate available space above and below the textarea
      var spaceAbove = textareaRect.top - safeMargin;
      var spaceBelow = viewportHeight - textareaRect.bottom - safeMargin;

      // Determine optimal position and max height
      var positionAbove = spaceAbove > spaceBelow && spaceAbove >= minDropdownHeight;
      var maxHeight = positionAbove ? spaceAbove : spaceBelow;

      // Clamp max height to reasonable bounds
      maxHeight = Math.min(maxHeight, 320); // Don't exceed 320px
      maxHeight = Math.max(maxHeight, minDropdownHeight); // At least show 3 items

      // Apply positioning
      this.dropdown.classList.toggle('position-below', !positionAbove);
      this.dropdown.style.maxHeight = maxHeight + 'px';

      // Update scroll indicators after positioning
      var self = this;
      setTimeout(function() { self.updateScrollIndicators(); }, 10);
    },

    /**
     * Update visual indicators showing if content is scrollable
     */
    updateScrollIndicators: function() {
      if (!this.dropdown) return;

      var scrollTop = this.dropdown.scrollTop;
      var scrollHeight = this.dropdown.scrollHeight;
      var clientHeight = this.dropdown.clientHeight;
      var threshold = 5; // Small threshold for float precision

      var canScrollUp = scrollTop > threshold;
      var canScrollDown = scrollTop + clientHeight < scrollHeight - threshold;

      this.dropdown.classList.toggle('can-scroll-up', canScrollUp);
      this.dropdown.classList.toggle('can-scroll-down', canScrollDown);
    },

    /**
     * Attach event listeners to the element
     */
    attachEventListeners: function() {
      var self = this;

      // Input handler for trigger detection
      this.element.addEventListener('input', function(e) {
        self.checkForTrigger();
      });

      // Keydown handler for navigation
      this.element.addEventListener('keydown', function(e) {
        self.onKeydown(e);
      });

      // Click outside to close
      document.addEventListener('click', function(e) {
        if (!self.dropdown.contains(e.target) && e.target !== self.element) {
          self.hide();
        }
      });

      // Handle blur with delay (allows clicking dropdown items)
      this.element.addEventListener('blur', function(e) {
        setTimeout(function() {
          // Skip if blur is suppressed (e.g., during agent switch)
          if (self.suppressBlur) {
            self.suppressBlur = false;
            return;
          }
          if (!self.dropdown.contains(document.activeElement)) {
            self.hide();
          }
        }, 150);
      });

      // Mouse wheel scrolling on dropdown - prevent page scroll when at limits
      this.dropdown.addEventListener('wheel', function(e) {
        var atTop = self.dropdown.scrollTop === 0;
        var atBottom = self.dropdown.scrollTop + self.dropdown.clientHeight >= self.dropdown.scrollHeight;

        // Prevent page scroll when dropdown can't scroll further
        if ((e.deltaY < 0 && atTop) || (e.deltaY > 0 && atBottom)) {
          e.preventDefault();
        }

        // Update scroll indicators
        self.updateScrollIndicators();
      }, { passive: false });

      // Update scroll indicators on scroll
      this.dropdown.addEventListener('scroll', function() {
        self.updateScrollIndicators();
      });

      // Reposition dropdown on window resize
      window.addEventListener('resize', function() {
        if (self.isVisible) {
          self.positionDropdown();
        }
      });
    },

    /**
     * Get text content from the element
     */
    getText: function() {
      if (this.isContentEditable) {
        return this.element.innerText || this.element.textContent || '';
      }
      return this.element.value || '';
    },

    /**
     * Get cursor position in the element
     */
    getCursorPosition: function() {
      if (this.isContentEditable) {
        var sel = window.getSelection();
        if (!sel.rangeCount) return 0;

        var range = sel.getRangeAt(0);
        // Only consider position if selection is within our element
        if (!this.element.contains(range.startContainer)) return 0;

        // Create a range from start of element to cursor
        var preRange = document.createRange();
        preRange.selectNodeContents(this.element);
        preRange.setEnd(range.startContainer, range.startOffset);

        // Get text length up to cursor
        return preRange.toString().length;
      }
      return this.element.selectionStart || 0;
    },

    /**
     * Load commands for the current agent
     * Uses dynamic commands from ACP if available, otherwise falls back to static data
     */
    loadCommands: function() {
      // Check for dynamic commands from ACP first
      var dynamicCommands = window.DynamicSlashCommands || {};
      if (dynamicCommands[this.agentId] && dynamicCommands[this.agentId].length > 0) {
        this.commands = dynamicCommands[this.agentId];
        console.log('[SlashCommands] Using', this.commands.length, 'dynamic commands for', this.agentId);
      } else {
        // Fall back to static data
        var data = window.SlashCommandsData || {};
        this.commands = data[this.agentId] || data['codex'] || [];
        console.log('[SlashCommands] Using', this.commands.length, 'static commands for', this.agentId);
      }
    },

    /**
     * Update commands dynamically from ACP
     * @param {Array} commands - Array of {name, description} objects from ACP
     */
    updateCommands: function(commands, agentId) {
      if (!Array.isArray(commands)) return;

      var targetAgent = agentId || this.agentId;

      // Store in global dynamic commands cache
      window.DynamicSlashCommands = window.DynamicSlashCommands || {};
      window.DynamicSlashCommands[targetAgent] = commands.map(function(cmd) {
        var name = cmd.name || '';
        return {
          name: name.startsWith('/') ? name : '/' + name,
          description: cmd.description || '',
          scope: cmd.scope || null
        };
      });

      console.log('[SlashCommands] Updated', commands.length, 'dynamic commands for', targetAgent);

      // Reload if this is the current agent
      if (targetAgent === this.agentId) {
        this.loadCommands();

        // Refresh dropdown if visible
        if (this.isVisible) {
          this.checkForTrigger();
        }
      }
    },

    /**
     * Set the active agent and reload commands
     * @param {string} agentId - The new agent ID
     */
    setAgent: function(agentId) {
      this.agentId = agentId;
      this.loadCommands();

      // Check if element contains a slash command trigger and re-open dropdown
      // This handles the case where clicking an agent tile closes the dropdown via blur
      var text = this.getText();

      // Find slash trigger position in text
      var slashPos = -1;
      for (var i = text.length - 1; i >= 0; i--) {
        if (text[i] === '/') {
          // Check if it's at start or after whitespace
          if (i === 0 || /\s/.test(text[i - 1])) {
            slashPos = i;
            break;
          }
        } else if (/\s/.test(text[i])) {
          break; // Stop at whitespace
        }
      }

      if (slashPos >= 0) {
        // There's a slash command in progress, show dropdown with new agent's commands
        // Suppress the pending blur event so it doesn't close our newly opened dropdown
        this.suppressBlur = true;
        this.triggerPosition = slashPos;
        var query = text.substring(slashPos + 1);
        this.show(query);
      }
    },

    /**
     * Check if the cursor is after a "/" trigger
     */
    checkForTrigger: function() {
      var cursorPos = this.getCursorPosition();
      var text = this.getText();

      // Find the start of the current "word" (slash command)
      var wordStart = cursorPos;
      while (wordStart > 0 && !/\s/.test(text[wordStart - 1])) {
        wordStart--;
      }

      var word = text.substring(wordStart, cursorPos);

      // Check if word starts with "/"
      if (word.startsWith('/')) {
        this.triggerPosition = wordStart;
        var query = word.substring(1); // Remove the leading "/"
        this.show(query);
      } else {
        this.hide();
      }
    },

    /**
     * Show the dropdown with filtered commands
     * @param {string} query - The search query (without leading "/")
     */
    show: function(query) {
      this.filterCommands(query);

      if (this.filteredCommands.length === 0) {
        this.hide();
        return;
      }

      this.selectedIndex = 0;
      this.render();
      this.positionDropdown();
      this.dropdown.style.display = 'block';
      this.isVisible = true;
    },

    /**
     * Hide the dropdown
     */
    hide: function() {
      this.dropdown.style.display = 'none';
      this.isVisible = false;
      this.triggerPosition = null;
    },

    /**
     * Filter commands based on query (fuzzy substring match)
     * @param {string} query - The search query
     */
    filterCommands: function(query) {
      var self = this;
      query = (query || '').toLowerCase();

      if (!query) {
        // Show ALL commands when just "/" is typed, sorted alphabetically
        this.filteredCommands = this.commands.slice().sort(function(a, b) {
          return a.name.localeCompare(b.name);
        });
      } else {
        // Filter commands that contain the query (case-insensitive)
        this.filteredCommands = this.commands.filter(function(cmd) {
          var name = cmd.name.toLowerCase();
          var desc = (cmd.description || '').toLowerCase();
          return name.includes(query) || desc.includes(query);
        });

        // Sort: exact prefix matches first, then alphabetically
        this.filteredCommands.sort(function(a, b) {
          var aName = a.name.toLowerCase();
          var bName = b.name.toLowerCase();
          var aPrefix = aName.startsWith('/' + query);
          var bPrefix = bName.startsWith('/' + query);

          if (aPrefix && !bPrefix) return -1;
          if (!aPrefix && bPrefix) return 1;
          return aName.localeCompare(bName);
        });
      }

      // No artificial limit - show all matching commands
    },

    /**
     * Render the dropdown content
     */
    render: function() {
      var self = this;
      var html = '';

      this.filteredCommands.forEach(function(cmd, index) {
        var selected = index === self.selectedIndex ? ' selected' : '';
        var scope = self.normalizeScope(cmd.scope);
        var scopeLabel = scope ? self.formatScopeLabel(scope) : '';
        html += '<div class="slash-command-item' + selected + '" data-index="' + index + '">';
        html += '<div class="command-name-group">';
        html += '<span class="command-name">' + self.escapeHtml(cmd.name) + '</span>';
        if (scope) {
          html += '<span class="command-scope scope-' + self.escapeHtml(scope) + '">' + self.escapeHtml(scopeLabel) + '</span>';
        }
        html += '</div>';
        html += '<span class="command-description">' + self.escapeHtml(cmd.description || '') + '</span>';
        html += '</div>';
      });

      this.dropdown.innerHTML = html;

      // Attach click handlers
      var items = this.dropdown.querySelectorAll('.slash-command-item');
      items.forEach(function(item) {
        item.addEventListener('mousedown', function(e) {
          e.preventDefault(); // Prevent blur
          var idx = parseInt(item.dataset.index, 10);
          self.selectCommand(idx);
        });
        item.addEventListener('mouseenter', function(e) {
          var idx = parseInt(item.dataset.index, 10);
          self.selectedIndex = idx;
          self.updateSelection();
        });
      });
    },

    /**
     * Update the visual selection
     */
    updateSelection: function() {
      var items = this.dropdown.querySelectorAll('.slash-command-item');
      items.forEach(function(item, idx) {
        item.classList.toggle('selected', idx === this.selectedIndex);
      }, this);

      // Scroll selected item into view with manual calculation for reliability
      var selected = this.dropdown.querySelector('.slash-command-item.selected');
      if (selected && this.dropdown) {
        var dropdownRect = this.dropdown.getBoundingClientRect();
        var selectedRect = selected.getBoundingClientRect();

        // Check if selected item is above visible area
        if (selectedRect.top < dropdownRect.top) {
          this.dropdown.scrollTop -= (dropdownRect.top - selectedRect.top);
        }
        // Check if selected item is below visible area
        else if (selectedRect.bottom > dropdownRect.bottom) {
          this.dropdown.scrollTop += (selectedRect.bottom - dropdownRect.bottom);
        }
      }
    },

    /**
     * Handle keydown events for navigation
     * @param {KeyboardEvent} e
     */
    onKeydown: function(e) {
      if (!this.isVisible) return;

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          this.selectedIndex = (this.selectedIndex + 1) % this.filteredCommands.length;
          this.updateSelection();
          break;

        case 'ArrowUp':
          e.preventDefault();
          this.selectedIndex = (this.selectedIndex - 1 + this.filteredCommands.length) % this.filteredCommands.length;
          this.updateSelection();
          break;

        case 'Tab':
        case 'Enter':
          if (this.filteredCommands.length > 0) {
            e.preventDefault();
            this.selectCommand(this.selectedIndex);
          }
          break;

        case 'Escape':
          e.preventDefault();
          this.hide();
          break;
      }
    },

    /**
     * Select a command and insert it into the element
     * @param {number} index - The index of the command to select
     */
    selectCommand: function(index) {
      if (index < 0 || index >= this.filteredCommands.length) return;

      var cmd = this.filteredCommands[index];
      var self = this;

      if (this.isContentEditable) {
        // For contenteditable, we need to manipulate the DOM
        var text = this.getText();
        var cursorPos = this.getCursorPosition();

        // Replace from trigger position to cursor with the command + space
        var before = text.substring(0, this.triggerPosition);
        var after = text.substring(cursorPos);
        var newText = before + cmd.name + ' ' + after;

        // Set the text content
        this.element.innerText = newText;

        // Position cursor after the inserted command + space
        var newCursorPos = this.triggerPosition + cmd.name.length + 1;
        this.setCursorPosition(newCursorPos);

        // Trigger input event for any listeners
        this.element.dispatchEvent(new Event('input', { bubbles: true }));
      } else {
        // For textarea
        var text = this.element.value;
        var cursorPos = this.element.selectionStart;

        // Replace from trigger position to cursor with the command + space
        var before = text.substring(0, this.triggerPosition);
        var after = text.substring(cursorPos);
        var newText = before + cmd.name + ' ' + after;

        this.element.value = newText;

        // Position cursor after the inserted command + space
        var newCursorPos = this.triggerPosition + cmd.name.length + 1;
        this.element.setSelectionRange(newCursorPos, newCursorPos);

        // Trigger input event for any listeners
        this.element.dispatchEvent(new Event('input', { bubbles: true }));
      }

      this.hide();
      this.element.focus();
    },

    /**
     * Set cursor position in contenteditable element
     */
    setCursorPosition: function(pos) {
      if (!this.isContentEditable) {
        this.element.setSelectionRange(pos, pos);
        return;
      }

      var node = this.element.firstChild;
      if (!node) {
        // Empty element, just focus
        this.element.focus();
        return;
      }

      // Handle text node
      if (node.nodeType === Node.TEXT_NODE) {
        var range = document.createRange();
        var sel = window.getSelection();
        var actualPos = Math.min(pos, node.textContent.length);
        range.setStart(node, actualPos);
        range.collapse(true);
        sel.removeAllRanges();
        sel.addRange(range);
      }
    },

    normalizeScope: function(scope) {
      if (!scope) return null;
      var value = scope.toString().toLowerCase();
      if (value === 'global' || value === 'user' || value === 'project') {
        return value;
      }
      return null;
    },

    formatScopeLabel: function(scope) {
      if (!scope) return '';
      return scope.charAt(0).toUpperCase() + scope.slice(1);
    },

    /**
     * Escape HTML to prevent XSS
     * @param {string} str
     * @returns {string}
     */
    escapeHtml: function(str) {
      if (!str) return '';
      var div = document.createElement('div');
      div.textContent = str;
      return div.innerHTML;
    },

    /**
     * Destroy the component and clean up
     */
    destroy: function() {
      if (this.dropdown && this.dropdown.parentElement) {
        this.dropdown.parentElement.removeChild(this.dropdown);
      }
      this.dropdown = null;
    }
  };

  // Export to global scope
  window.SlashCommandAutocomplete = SlashCommandAutocomplete;
})();
