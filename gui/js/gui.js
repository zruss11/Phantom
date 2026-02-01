/*******************************
        Navigation Script
 ******************************/
let navLinks = $(".nav-link");
navLinks.click(function () {
  if ($(this).attr("id") === "minimize-icon") {
    return;
  }

  let page = $(this).attr("data-page");

  $(".nav-link").each(function () {
    $(this).parent().removeClass("active");
  });

  $(this).parent().addClass("active");

  $(".page").each(function () {
    $(this).attr("hidden", true);
  });

  $("#" + page).attr("hidden", false);

  // Fire a navigation event so other modules can react without polling.
  try {
    window.dispatchEvent(
      new CustomEvent("PhantomNavigate", { detail: { pageId: page } }),
    );
  } catch (e) {
    // ignore
  }

  // Auto-focus prompt input when navigating to Create Tasks (Notion-style UX)
  if (page === "createTasksPage") {
    requestAnimationFrame(() => {
      const prompt = document.getElementById("initialPrompt");
      if (prompt) {
        prompt.focus();
      }
    });
    if (typeof window.hydrateModelsFromCache === "function") {
      window.hydrateModelsFromCache();
    }
  }
});
/*******************************
      Keyboard Shortcuts
 ******************************/
// Switch to a page by its ID
function switchToPage(pageId) {
  const navLink = $(`a[data-page="${pageId}"]`);
  if (navLink.length) {
    // Remove active from all nav items
    $(".nav-link").each(function () {
      $(this).parent().removeClass("active");
    });
    // Add active to target nav item
    navLink.parent().addClass("active");
    // Hide all pages
    $(".page").each(function () {
      $(this).attr("hidden", true);
    });
    // Show target page
    $("#" + pageId).attr("hidden", false);

    // Fire a navigation event so other modules can react without polling.
    try {
      window.dispatchEvent(
        new CustomEvent("PhantomNavigate", { detail: { pageId: pageId } }),
      );
    } catch (e) {
      // ignore
    }

    // Auto-focus prompt input when navigating to Create Tasks (Notion-style UX)
    if (pageId === "createTasksPage") {
      requestAnimationFrame(() => {
        const prompt = document.getElementById("initialPrompt");
        if (prompt) {
          prompt.focus();
        }
      });
      if (typeof window.hydrateModelsFromCache === "function") {
        window.hydrateModelsFromCache();
      }
    }
  }
}

// Keyboard shortcuts are now handled by the keybinds module in application.js
// See initKeybinds() for customizable keyboard shortcuts

/*******************************
      Profile Page Script
 ******************************/
let billingShippingMatch = $("#billingShippingMatch");
// Billing Info elements
let billingFirstName = $("#billingFirstName");
let billingLastName = $("#billingLastName");
let billingAddress = $("#billingAddress");
let billingAptSuite = $("#billingAptSuite");
let billingCity = $("#billingCity");
let billingState = $("#billingState");
let billingPostalCode = $("#billingPostalCode");
let billingElArray = [
  billingFirstName,
  billingLastName,
  billingAddress,
  billingAptSuite,
  billingCity,
  billingState,
  billingPostalCode,
];

billingShippingMatch.click(function () {
  let billingShippingMatchIsChecked = billingShippingMatch.is(":checked");
  if (billingShippingMatchIsChecked) {
    // Disable Billing
    $.each(billingElArray, function () {
      $(this).prop("disabled", true);
    });
  } else {
    $.each(billingElArray, function () {
      $(this).prop("disabled", false);
    });
  }
});

/*******************************
 Profiles Page Script
 ******************************/
let shippingCountry = $("#shippingCountry");
let stateCol = $("#shippingStateCol");
let cityCol = $("#shippingCityCol");
shippingCountry.change(function () {
  let selected = $("#shippingCountry").val();
  if (selected === "US") {
    // show states
    cityCol.removeClass("col-sm-8");
    cityCol.addClass("col-sm-4");
    stateCol.show();
  } else if (selected == "CA" || selected == "AU") {
    cityCol.removeClass("col-sm-8");
    cityCol.addClass("col-sm-4");
    stateCol.show();
  } else {
    // hide states
    stateCol.hide();
    cityCol.removeClass("col-sm-4");
    cityCol.addClass("col-sm-8");
  }
});

/*******************************
 Settings Page Script
 ******************************/
let copyButton = $("#copyInstanceCode");

copyButton.click(function () {
  let copyText = $("#singleUseInstanceCode");

  /* Select the text field */
  copyText.select();

  /* Copy the text inside the text field */
  document.execCommand("Copy");
});

$(function () {
  $('[data-toggle="tooltip"]').tooltip();

  // Handle navigation from other pages (e.g., returning from skills-tree.html)
  const navigateTo = sessionStorage.getItem('navigateTo');
  if (navigateTo) {
    sessionStorage.removeItem('navigateTo');
    // Wait for page to fully initialize before switching
    setTimeout(function() {
      if (typeof switchToPage === 'function') {
        switchToPage(navigateTo);
      } else {
        const navEl = document.querySelector('[data-page="' + navigateTo + '"]');
        if (navEl) navEl.click();
      }
    }, 100);
  }
});

//upcomingReleaseLoading();
