#!/usr/bin/env bash
# Main demo runner

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --pace)
            export DEMO_PACE="$2"
            shift 2
            ;;
        --interactive)
            export DEMO_INTERACTIVE=1
            shift
            ;;
        --live)
            export DEMO_LIVE=1
            shift
            ;;
        --scenario)
            SCENARIO="$2"
            shift 2
            ;;
        --help)
            echo "Usage: demo.sh [OPTIONS]"
            echo
            echo "Options:"
            echo "  --pace SECONDS     Wait between steps (default: 0)"
            echo "  --interactive      Wait for Enter between steps"
            echo "  --live             Use real waybar paths (updates your actual waybar)"
            echo "  --scenario NAME    Run specific scenario (single-session, multi-session)"
            echo
            echo "Scenarios:"
            echo "  single-session     Basic tool activity progression"
            echo "  multi-session      Aggregate view from multiple sessions"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

check_binary

echo -e "${CYAN}"
echo "╔═══════════════════════════════════════╗"
echo "║     llm-waybar Demo                   ║"
echo "║     Claude Code → Waybar Bridge       ║"
echo "╚═══════════════════════════════════════╝"
echo -e "${NC}"

if [[ -n "$SCENARIO" ]]; then
    case $SCENARIO in
        single-session)
            "$SCRIPT_DIR/scenarios/single-session.sh"
            ;;
        multi-session)
            "$SCRIPT_DIR/scenarios/multi-session.sh"
            ;;
        *)
            echo "Unknown scenario: $SCENARIO"
            exit 1
            ;;
    esac
else
    # Run all scenarios
    "$SCRIPT_DIR/scenarios/single-session.sh"
    echo
    "$SCRIPT_DIR/scenarios/multi-session.sh"
fi

echo -e "\n${GREEN}═══ All demos complete! ═══${NC}\n"
